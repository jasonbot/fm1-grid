//! The FM-1 control map: which computer key plays which FM-1 key/button, and the layout
//! of the miniature front panel (function rows, note keys). Pure data, no logic.
//! Computer keys are gpui keystroke names (`KeyDownEvent::keystroke.key`): lowercase
//! letters, digits, and the literal punctuation keys.

/// The computer-keyboard layout shared with fm1-live: C4..F5 on the piano keys. The low
/// octave F3..B3 uses the seven letters nothing else claims: the bottom-row run C V B N M
/// (below the home row, like F3..B3 sits below C4), then Q and I for the top two notes.
pub const PIANO: [(&str, u8); 25] = [
    ("c", 14),
    ("v", 15),
    ("b", 16),
    ("n", 17),
    ("m", 18),
    ("q", 19),
    ("i", 20),
    ("a", 21),
    ("w", 22),
    ("s", 23),
    ("e", 24),
    ("d", 25),
    ("f", 26),
    ("t", 27),
    ("g", 28),
    ("y", 29),
    ("h", 30),
    ("u", 31),
    ("j", 32),
    ("k", 33),
    ("o", 34),
    ("l", 35),
    ("p", 36),
    (";", 37),
    ("'", 38),
];

/// The FM-1's octave and function buttons.
pub const BUTTONS: [(&str, u8); 14] = [
    ("z", 0),
    ("x", 1),
    ("1", 2),
    ("2", 3),
    ("3", 4),
    ("4", 5),
    ("5", 6),
    ("6", 7),
    ("7", 8),
    ("8", 9),
    ("9", 10),
    ("0", 11),
    ("space", 12),
    ("r", 13),
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
