//! The FM-1 control map: which computer key plays which FM-1 key/button, and the layout
//! of the miniature front panel (function rows, note keys). Pure data, no logic.

use eframe::egui;

/// The computer-keyboard layout shared with fm1-live: C4..F5 on the piano keys. The low
/// octave F3..B3 uses the seven letters nothing else claims: the bottom-row run C V B N M
/// (below the home row, like F3..B3 sits below C4), then Q and I for the top two notes.
pub const PIANO: [(egui::Key, u8); 25] = [
    (egui::Key::C, 14),
    (egui::Key::V, 15),
    (egui::Key::B, 16),
    (egui::Key::N, 17),
    (egui::Key::M, 18),
    (egui::Key::Q, 19),
    (egui::Key::I, 20),
    (egui::Key::A, 21),
    (egui::Key::W, 22),
    (egui::Key::S, 23),
    (egui::Key::E, 24),
    (egui::Key::D, 25),
    (egui::Key::F, 26),
    (egui::Key::T, 27),
    (egui::Key::G, 28),
    (egui::Key::Y, 29),
    (egui::Key::H, 30),
    (egui::Key::U, 31),
    (egui::Key::J, 32),
    (egui::Key::K, 33),
    (egui::Key::O, 34),
    (egui::Key::L, 35),
    (egui::Key::P, 36),
    (egui::Key::Semicolon, 37),
    (egui::Key::Quote, 38),
];

/// The FM-1's octave and function buttons.
pub const BUTTONS: [(egui::Key, u8); 14] = [
    (egui::Key::Z, 0),
    (egui::Key::X, 1),
    (egui::Key::Num1, 2),
    (egui::Key::Num2, 3),
    (egui::Key::Num3, 4),
    (egui::Key::Num4, 5),
    (egui::Key::Num5, 6),
    (egui::Key::Num6, 7),
    (egui::Key::Num7, 8),
    (egui::Key::Num8, 9),
    (egui::Key::Num9, 10),
    (egui::Key::Num0, 11),
    (egui::Key::Space, 12),
    (egui::Key::R, 13),
];

/// The FM-1's two rows of six function buttons: (label, key id, computer key).
pub const FUNCTION_ROW_1: [(&str, u8, &str); 6] = [
    ("FX", 2, "1"),
    ("SCL", 3, "2"),
    ("ENV", 4, "3"),
    ("LFO", 5, "4"),
    ("EDIT", 6, "5"),
    ("GLO", 7, "6"),
];
pub const FUNCTION_ROW_2: [(&str, u8, &str); 6] = [
    ("HOME", 8, "7"),
    ("SAVE", 9, "8"),
    ("ARP", 10, "9"),
    ("SEQ", 11, "0"),
    ("PLAY", 12, "SPACE"),
    ("REC", 13, "R"),
];

/// The FM-1's 27 note keys, F3..G5: (key id, note name, black?).
pub const PANEL_KEYS: [(u8, &str, bool); 27] = [
    (14, "F3", false),
    (15, "F#3", true),
    (16, "G3", false),
    (17, "G#3", true),
    (18, "A3", false),
    (19, "A#3", true),
    (20, "B3", false),
    (21, "C4", false),
    (22, "C#4", true),
    (23, "D4", false),
    (24, "D#4", true),
    (25, "E4", false),
    (26, "F4", false),
    (27, "F#4", true),
    (28, "G4", false),
    (29, "G#4", true),
    (30, "A4", false),
    (31, "A#4", true),
    (32, "B4", false),
    (33, "C5", false),
    (34, "C#5", true),
    (35, "D5", false),
    (36, "D#5", true),
    (37, "E5", false),
    (38, "F5", false),
    (39, "F#5", true),
    (40, "G5", false),
];

/// White keys across the panel, left to right.
pub const WHITE_KEYS: [u8; 16] = [14, 16, 18, 20, 21, 23, 25, 26, 28, 30, 32, 33, 35, 37, 38, 40];

/// Black keys: (key id, index of the white key they sit after).
pub const BLACK_KEYS: [(u8, usize); 11] = [
    (15, 0),
    (17, 1),
    (19, 2),
    (22, 4),
    (24, 5),
    (27, 7),
    (29, 8),
    (31, 9),
    (34, 11),
    (36, 12),
    (39, 14),
];

/// The computer key that plays a panel key, if it has one.
pub fn panel_key_hint(id: u8) -> &'static str {
    match id {
        14 => "C",
        15 => "V",
        16 => "B",
        17 => "N",
        18 => "M",
        19 => "Q",
        20 => "I",
        21 => "A",
        22 => "W",
        23 => "S",
        24 => "E",
        25 => "D",
        26 => "F",
        27 => "T",
        28 => "G",
        29 => "Y",
        30 => "H",
        31 => "U",
        32 => "J",
        33 => "K",
        34 => "O",
        35 => "L",
        36 => "P",
        37 => ";",
        38 => "'",
        _ => "",
    }
}
