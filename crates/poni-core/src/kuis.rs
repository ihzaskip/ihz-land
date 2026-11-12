//! The Kuis page: one JISPA math question at a time from ihz.app, through
//! the signed-in account (`GET /api/jispa/kuis`, the device key as Bearer).
//! ihz.app renders every formula to SVG (MathJax there), so the app carries
//! no TeX engine: it draws the SVG with Direct2D (`gfx::Canvas::svg`).
//! The answer comes with the question; the app marks it on the spot.

use serde_json::Value;

pub fn url(base: &str) -> String {
    format!("{base}/api/jispa/kuis")
}

/// One question. `prompt`, `choices` and `solution` are SVG documents
/// (`svg`, `g`, `path`, `rect`, sized in px, painted `currentColor`);
/// `lead` and `note` are plain text.
#[derive(Clone, Debug, PartialEq)]
pub struct Question {
    pub topic_label: String,
    pub lead: Option<String>,
    pub prompt: String,
    pub choices: Vec<String>,
    pub answer: usize,
    pub solution: Vec<String>,
    pub note: Option<String>,
}

/// The question in `body`, if it is one the page can show.
// ponytail: read through `Value` like `ielts::parse_me`, not a serde
// derive: the exe has that code already (a derive cost ~8 KB more).
pub fn parse(body: &str) -> Option<Question> {
    let v: Value = serde_json::from_str(body).ok()?;
    let text = |k: &str| v[k].as_str().map(str::to_string);
    let list = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect::<Vec<_>>());
    let q = Question {
        topic_label: text("topicLabel")?,
        lead: text("lead"),
        prompt: text("prompt")?,
        choices: list("choices")?,
        answer: v["answer"].as_u64()? as usize,
        solution: list("solution").unwrap_or_default(),
        note: text("note"),
    };
    let ok = (2..=6).contains(&q.choices.len()) && q.answer < q.choices.len() && svg_size(&q.prompt).is_some() && q.choices.iter().all(|c| svg_size(c).is_some());
    ok.then_some(q)
}

/// The width and height (px) on an SVG's root element.
pub fn svg_size(svg: &str) -> Option<(f32, f32)> {
    let head = svg.get(..svg.find('>')?)?.strip_prefix("<svg")?;
    let attr = |name: &str| -> Option<f32> {
        let at = head.find(&format!(" {name}=\""))? + name.len() + 3;
        let rest = &head[at..];
        rest[..rest.find('"')?].parse().ok().filter(|v: &f32| v.is_finite() && *v > 0.0)
    };
    Some((attr("width")?, attr("height")?))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// 401, or no ihz.app key on this PC.
    SignedOut,
    /// No answer: no internet, or ihz.app didn't answer in time.
    Offline,
    /// 429: too many questions this minute.
    TooFast,
    /// Anything else (a 5xx, a body that isn't a question).
    Server,
}

impl Problem {
    pub fn from_status(status: u16) -> Problem {
        match status {
            401 => Problem::SignedOut,
            429 => Problem::TooFast,
            _ => Problem::Server,
        }
    }

    pub fn line(self) -> &'static str {
        match self {
            Problem::SignedOut => "ihz.app nggak kenal PC ini lagi. Masuk lagi dari Settings.",
            Problem::Offline => "Nggak nyambung ke ihz.app. Cek internetnya, terus coba lagi.",
            Problem::TooFast => "Kebanyakan soal semenit ini. Tunggu sebentar, ya.",
            Problem::Server => "ihz.app lagi bermasalah. Coba lagi sebentar lagi.",
        }
    }
}

/// The page's state. Nothing runs at rest: a question is asked for only
/// when the page opens without one, or on Soal berikutnya.
#[derive(Clone, Debug, Default)]
pub struct Quiz {
    pub question: Option<Question>,
    /// The choice clicked (the question is then marked).
    pub picked: Option<usize>,
    /// A question is on its way.
    pub loading: bool,
    pub problem: Option<Problem>,
}

impl Quiz {
    /// The page wants a question now (opened without one, nothing failed).
    pub fn wants_first(&self) -> bool {
        self.question.is_none() && !self.loading && self.problem.is_none()
    }

    /// The answer to a request.
    pub fn came(&mut self, got: Result<Question, Problem>) {
        self.loading = false;
        match got {
            Ok(q) => {
                self.question = Some(q);
                self.picked = None;
                self.problem = None;
            }
            Err(p) => self.problem = Some(p),
        }
    }

    /// Whether the picked choice was right (None before a pick).
    pub fn right(&self) -> Option<bool> {
        Some(self.picked? == self.question.as_ref()?.answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real response, made by ihz.app's route from the JISPA engine.
    const SAMPLE: &str = include_str!("../data/kuis-sample.json");

    #[test]
    fn parses_a_real_response() {
        let q = parse(SAMPLE).expect("the sample is a question");
        assert_eq!(q.topic_label, "Integral");
        assert_eq!(q.choices.len(), 5);
        assert!(q.answer < 5);
        let (w, h) = svg_size(&q.prompt).unwrap();
        assert!(w > 10.0 && h > 10.0, "{w}x{h}");
        assert!(q.note.is_some() && !q.solution.is_empty());
    }

    #[test]
    fn refuses_what_it_cannot_show() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10.5" height="8" viewBox="0 0 1 1"></svg>"#;
        let body = |answer: usize, prompt: &str| {
            serde_json::json!({ "topicLabel": "X", "prompt": prompt, "choices": [svg, svg], "answer": answer }).to_string()
        };
        assert_eq!(svg_size(svg), Some((10.5, 8.0)));
        assert!(parse(&body(1, svg)).is_some());
        assert!(parse(&body(2, svg)).is_none(), "answer out of range");
        assert!(parse(&body(0, r#"<svg width="3ex" height="1ex">"#)).is_none(), "ex units");
        assert!(parse("<html>").is_none());
    }

    #[test]
    fn marks_and_moves_on() {
        let mut quiz = Quiz::default();
        assert!(quiz.wants_first());
        quiz.loading = true;
        assert!(!quiz.wants_first());
        quiz.came(Err(Problem::from_status(401)));
        assert_eq!(quiz.problem, Some(Problem::SignedOut));
        assert!(!quiz.wants_first(), "a failure waits for Coba lagi");
        quiz.came(Ok(parse(SAMPLE).unwrap()));
        let answer = quiz.question.as_ref().unwrap().answer;
        assert_eq!(quiz.right(), None);
        quiz.picked = Some(answer);
        assert_eq!(quiz.right(), Some(true));
        quiz.picked = Some((answer + 1) % 5);
        assert_eq!(quiz.right(), Some(false));
    }
}
