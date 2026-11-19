//! GTA VI on AnyPS5: the one place its status is set. The GTA VI page, the main screen's GTA VI scene,
//! its call to action and `!game gta 6` all read it from `/api/state` (`gta6`).
//!
//! When the game gets dumped, set STAGE to `Stage::Dumped`; once it plays on PC, `Stage::Playable`.
//! Update the firmware figures when Sony or the jailbreak scene moves.
use serde_json::{Value, json};

// Only one stage is set at a time: the others wait for the game to move on.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Out on PS5, but no one can dump it: no public jailbreak reaches the firmware it needs.
    Released,
    /// Dumped from a PS5: AnyPS5 can try to convert it.
    Dumped,
    /// Playable on PC with AnyPS5.
    Playable,
}

pub const STAGE: Stage = Stage::Released;
/// It launched at midnight local time on 19 November 2026, New Zealand first (UTC+13).
pub const RELEASED_AT: &str = "2026-11-18T11:00:00Z";
/// The PS5 system software GTA VI requires: 26.07-14.23.00, out on 17 November, made mandatory by Sony on 18 November.
pub const REQUIRED_FIRMWARE: &str = "14.23";
/// The highest firmware a public PS5 jailbreak reaches: Relapse, tethered, 7.00 to 13.60.
pub const JAILBREAK_FIRMWARE: &str = "13.60";

const STEPS: [&str; 3] = ["Out on PS5", "Dumped", "Playable on PC"];

/// GTA VI's status in a line of the chat's "about" message.
pub fn chat_line() -> String {
    chat_line_at(STAGE)
}

fn chat_line_at(stage: Stage) -> String {
    match stage {
        Stage::Released => format!(
            "No work on GTA VI yet: waiting for a dump (it needs PS5 firmware {REQUIRED_FIRMWARE})."
        ),
        Stage::Dumped => "GTA VI is dumped: AnyPS5 tests have started.".into(),
        Stage::Playable => "GTA VI is playable on PC with AnyPS5.".into(),
    }
}

/// Before a dump, no one can have tested it: a GTA VI test thread is a placeholder (the one opened on 15 November
/// says "Does not convert" without a try), so the status above wins over it.
pub fn reports_count() -> bool {
    STAGE != Stage::Released
}

pub fn status() -> Value {
    status_at(STAGE)
}

fn status_at(stage: Stage) -> Value {
    let (key, step, label, title, detail) = match stage {
        Stage::Released => (
            "released",
            0,
            "Not dumped",
            "Out on PS5, not dumped yet".to_owned(),
            format!(
                "It needs PS5 firmware {REQUIRED_FIRMWARE}, and the public jailbreak stops at {JAILBREAK_FIRMWARE}: no one can dump the game yet, and AnyPS5 needs a dump to convert it."
            ),
        ),
        Stage::Dumped => (
            "dumped",
            1,
            "Dumped",
            "Dumped, AnyPS5 tests can start".to_owned(),
            "The game has been dumped from a PS5: AnyPS5 can now try to convert it.".to_owned(),
        ),
        Stage::Playable => (
            "playable",
            2,
            "Playable",
            "Playable on PC with AnyPS5".to_owned(),
            "AnyPS5 converts the PS5 game to run natively on PC.".to_owned(),
        ),
    };
    json!({
        "stage": key,
        "step": step,
        "steps": STEPS,
        "label": label,
        "title": title,
        "detail": detail,
        "releasedAt": RELEASED_AT,
        "reportsCount": stage != Stage::Released,
        "firmware": {"required": REQUIRED_FIRMWARE, "jailbreak": JAILBREAK_FIRMWARE},
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_stage_names_its_step_and_keeps_the_firmware_figures() {
        let released = status_at(Stage::Released);
        assert_eq!(released["stage"], "released");
        assert_eq!(released["step"], 0);
        assert_eq!(released["steps"].as_array().unwrap().len(), 3);
        let detail = released["detail"].as_str().unwrap();
        assert!(detail.contains(REQUIRED_FIRMWARE) && detail.contains(JAILBREAK_FIRMWARE));
        assert_eq!(status_at(Stage::Dumped)["step"], 1);
        assert_eq!(status_at(Stage::Playable)["step"], 2);
        assert_eq!(released["firmware"]["required"], "14.23");
        assert!(chrono::DateTime::parse_from_rfc3339(RELEASED_AT).is_ok());
    }

    #[test]
    fn the_about_message_fits_a_chat_message_at_every_stage() {
        for stage in [Stage::Released, Stage::Dumped, Stage::Playable] {
            let about = crate::chat::about_message(&chat_line_at(stage));
            assert!(
                about.chars().count() <= 200,
                "{} chars: {about}",
                about.chars().count()
            );
            assert!(about.contains("https://github.com/simnJS/AnyPS5"));
        }
        assert!(chat_line_at(Stage::Released).contains("waiting for a dump"));
    }
}
