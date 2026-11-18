//! The keyboard of a terminal as the keyboard of a window.
//!
//! A text console, like every terminal, sends the characters and the escape sequences of the
//! keys that are pressed, and repeats them while a key is held. It does not say when a key is
//! released: `Holds` makes the releases up.

use crate::event::{KeyCode, KeyMods};
use std::time::{Duration, Instant};

/// A key that the terminal reported, with the character that it typed if it types one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub key: KeyCode,
    pub mods: KeyMods,
    pub character: Option<char>,
}

fn key(key: KeyCode) -> Stroke {
    Stroke {
        key,
        mods: KeyMods::default(),
        character: None,
    }
}

/// How long an escape character waits for the rest of its sequence. Alone it is the Escape key.
const ESCAPE: Duration = Duration::from_millis(50);

/// Turns the bytes of a terminal into keys.
#[derive(Default)]
pub struct Parser {
    /// An escape sequence, or a character of several bytes, that has not ended yet.
    pending: Vec<u8>,
    started: Option<Instant>,
}

impl Parser {
    /// The keys of `bytes`, which were read at `now`.
    pub fn feed(&mut self, bytes: &[u8], now: Instant) -> Vec<Stroke> {
        let mut strokes = self.expire(now);
        for &byte in bytes {
            match self.pending.first().copied() {
                Some(0x1b) => self.escape(byte, now, &mut strokes),
                Some(_) => {
                    self.pending.push(byte);
                    self.character(&mut strokes);
                }
                None if byte == 0x1b || byte >= 0x80 => {
                    self.pending.push(byte);
                    self.started = Some(now);
                    self.character(&mut strokes);
                }
                None => strokes.extend(plain(byte, KeyMods::default())),
            }
        }
        strokes
    }

    /// The Escape key, when its character has waited long enough for a sequence. One that
    /// has not ended by then is forgotten.
    pub fn expire(&mut self, now: Instant) -> Vec<Stroke> {
        match self.started {
            Some(started) if now.duration_since(started) >= ESCAPE => {
                let escape = self.pending == [0x1b];
                self.clear();
                if escape {
                    vec![key(KeyCode::Escape)]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.pending.clear();
        self.started = None;
    }

    /// Ends a character of several bytes when it is whole.
    fn character(&mut self, strokes: &mut Vec<Stroke>) {
        if self.pending == [0x1b] {
            return;
        }
        match std::str::from_utf8(&self.pending) {
            Ok(text) => {
                strokes.extend(text.chars().map(|character| Stroke {
                    character: Some(character),
                    ..key(KeyCode::Unknown)
                }));
                self.clear();
            }
            Err(error) if error.error_len().is_some() => self.clear(),
            Err(_) => {}
        }
    }

    fn escape(&mut self, byte: u8, now: Instant, strokes: &mut Vec<Stroke>) {
        if self.pending.len() == 1 {
            match byte {
                b'[' | b'O' => self.pending.push(byte),
                0x1b => {
                    strokes.push(key(KeyCode::Escape));
                    self.started = Some(now);
                }
                // A terminal sends Alt with a key as an escape character before the key.
                _ => {
                    self.clear();
                    let alt = KeyMods {
                        alt: true,
                        ..KeyMods::default()
                    };
                    strokes.extend(plain(byte, alt));
                }
            }
            return;
        }
        self.pending.push(byte);
        // The Linux console sends F1 to F5 as `ESC [ [ A` to `ESC [ [ E`.
        if self.pending == b"\x1b[[" {
            return;
        }
        if (0x40..=0x7e).contains(&byte) {
            strokes.extend(sequence(&self.pending));
            self.clear();
        } else if self.pending.len() > 32 {
            self.clear();
        }
    }
}

/// The key of one byte that is not part of an escape sequence.
fn plain(byte: u8, mods: KeyMods) -> Option<Stroke> {
    let stroke = |key, mods, character| {
        Some(Stroke {
            key,
            mods,
            character,
        })
    };
    match byte {
        b'\r' | b'\n' => stroke(KeyCode::Enter, mods, None),
        b'\t' => stroke(KeyCode::Tab, mods, None),
        0x08 | 0x7f => stroke(KeyCode::Backspace, mods, None),
        0x20..=0x7e => {
            let (key, shift) = typed(byte)?;
            stroke(key, KeyMods { shift, ..mods }, Some(byte as char))
        }
        // Control with a letter.
        0x01..=0x1a => {
            let (key, _) = typed(byte + 0x60)?;
            stroke(key, KeyMods { ctrl: true, ..mods }, None)
        }
        _ => None,
    }
}

/// The key that types a character on a keyboard with the US layout, and whether with Shift.
fn typed(byte: u8) -> Option<(KeyCode, bool)> {
    use KeyCode::*;
    const LETTERS: [KeyCode; 26] = [
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    ];
    const DIGITS: [KeyCode; 10] = [Key0, Key1, Key2, Key3, Key4, Key5, Key6, Key7, Key8, Key9];
    Some(match byte {
        b'a'..=b'z' => (LETTERS[usize::from(byte - b'a')], false),
        b'A'..=b'Z' => (LETTERS[usize::from(byte - b'A')], true),
        b'0'..=b'9' => (DIGITS[usize::from(byte - b'0')], false),
        b' ' => (Space, false),
        b'!' => (Key1, true),
        b'@' => (Key2, true),
        b'#' => (Key3, true),
        b'$' => (Key4, true),
        b'%' => (Key5, true),
        b'^' => (Key6, true),
        b'&' => (Key7, true),
        b'*' => (Key8, true),
        b'(' => (Key9, true),
        b')' => (Key0, true),
        b'-' => (Minus, false),
        b'_' => (Minus, true),
        b'=' => (Equal, false),
        b'+' => (Equal, true),
        b'[' => (LeftBracket, false),
        b'{' => (LeftBracket, true),
        b']' => (RightBracket, false),
        b'}' => (RightBracket, true),
        b'\\' => (Backslash, false),
        b'|' => (Backslash, true),
        b';' => (Semicolon, false),
        b':' => (Semicolon, true),
        b'\'' => (Apostrophe, false),
        b'"' => (Apostrophe, true),
        b',' => (Comma, false),
        b'<' => (Comma, true),
        b'.' => (Period, false),
        b'>' => (Period, true),
        b'/' => (Slash, false),
        b'?' => (Slash, true),
        b'`' => (GraveAccent, false),
        b'~' => (GraveAccent, true),
        _ => return None,
    })
}

fn function(number: u8) -> Option<KeyCode> {
    use KeyCode::*;
    [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12]
        .get(usize::from(number).checked_sub(1)?)
        .copied()
}

/// The key of a whole escape sequence: xterm's, which the FreeBSD console and most terminals
/// send, the Linux console's, and those of the old FreeBSD console (cons25).
fn sequence(sequence: &[u8]) -> Option<Stroke> {
    use KeyCode::*;
    let kind = sequence[1];
    let (parameters, last) = sequence[2..].split_at(sequence.len() - 3);
    let last = last[0];
    if kind == b'[' && parameters == b"[" {
        return if (b'A'..=b'E').contains(&last) {
            function(last - b'A' + 1).map(key)
        } else {
            None
        };
    }
    let mut numbers = parameters
        .split(|byte| *byte == b';')
        .map(|digits| std::str::from_utf8(digits).ok()?.parse::<u32>().ok());
    let first = numbers.next().flatten();
    let second = numbers.next().flatten();
    let found = match (kind, last) {
        (_, b'A') => Up,
        (_, b'B') => Down,
        (_, b'C') => Right,
        (_, b'D') => Left,
        (_, b'H') => Home,
        (_, b'F') => End,
        (b'O', b'P'..=b'S') => function(last - b'P' + 1)?,
        (b'[', b'P'..=b'S') if first == Some(1) => function(last - b'P' + 1)?,
        (b'[', b'~') => match first? {
            1 | 7 => Home,
            2 => Insert,
            3 => Delete,
            4 | 8 => End,
            5 => PageUp,
            6 => PageDown,
            number @ 11..=15 => function(number as u8 - 10)?,
            number @ 17..=21 => function(number as u8 - 11)?,
            number @ 23..=24 => function(number as u8 - 12)?,
            _ => return None,
        },
        (b'[', b'M'..=b'X') if parameters.is_empty() => function(last - b'M' + 1)?,
        (b'[', b'L') if parameters.is_empty() => Insert,
        (b'[', b'I') if parameters.is_empty() => PageUp,
        (b'[', b'G') if parameters.is_empty() => PageDown,
        _ => return None,
    };
    // xterm: the second number is one more than the sum of Shift 1, Alt 2 and Control 4.
    let held = second.map_or(0, |number| number.saturating_sub(1));
    Some(Stroke {
        mods: KeyMods {
            shift: held & 1 != 0,
            alt: held & 2 != 0,
            ctrl: held & 4 != 0,
            logo: false,
        },
        ..key(found)
    })
}

/// How long a key is held after a character that no other one of that key came before: a tap.
const TAP: Duration = Duration::from_millis(40);
/// The pause between two repeats of a key, until the terminal tells it or shows it.
const REPEAT: Duration = Duration::from_millis(130);
/// The longest hold after one character.
const LONGEST: Duration = Duration::from_millis(400);

/// The keys that are held, as far as the characters of a terminal tell.
///
/// A character presses its key. A key is released when its next repeat is overdue: after a
/// tap soon, and while it repeats a little later than the repeats come. So a tap is a short
/// press, and a held key is a short press, a pause as long as the terminal waits before it
/// repeats, and then one long press.
///
/// The character that ends that pause could also be a second tap. It holds the key without
/// pressing it again, as a repeat does; when no repeat follows it, it was a tap, and its
/// press comes then. So a key that is held is pressed once, and two taps are two presses.
pub struct Holds {
    keys: Vec<Held>,
    /// The pause between two repeats.
    repeat: Duration,
    /// How long after a character the first repeat of its key can come: soonest and latest.
    first: (Duration, Duration),
}

struct Held {
    key: KeyCode,
    mods: KeyMods,
    /// When the last character of the key came.
    last: Instant,
    /// When the key is released unless another character comes.
    until: Instant,
    down: bool,
    /// Whether the character that holds the key is its first repeat or a second tap.
    doubt: bool,
}

/// What became of a key whose hold is over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ended {
    /// It is released.
    Released(KeyCode, KeyMods),
    /// It was tapped a second time, which looked like the first repeat of a held key until
    /// no repeat followed: it is released and pressed again.
    Tapped(KeyCode, KeyMods),
}

impl Holds {
    /// `rate`: how long the terminal waits before it repeats a key, and between two repeats,
    /// if it tells.
    pub fn new(rate: Option<(Duration, Duration)>) -> Self {
        let (first, repeat) = match rate {
            Some((delay, repeat)) => ((delay * 3 / 4, delay * 3 / 2), repeat),
            // Any pause up to that of a slow terminal.
            None => ((Duration::ZERO, Duration::from_millis(990)), REPEAT),
        };
        Self {
            keys: Vec::new(),
            repeat,
            first,
        }
    }

    /// A character of `key` came at `now`. `true`: the key counts as held, so this is a
    /// repeat and no new press.
    pub fn press(&mut self, key: KeyCode, mods: KeyMods, now: Instant) -> bool {
        let stretch = |pause: Duration| (pause * 3 / 2 + Duration::from_millis(20)).min(LONGEST);
        match self.keys.iter_mut().find(|held| held.key == key) {
            Some(held) => {
                let pause = now.duration_since(held.last);
                let first = !held.down && (self.first.0..=self.first.1).contains(&pause);
                let repeated = held.down || first;
                held.until = now
                    + if held.down {
                        stretch(pause)
                    } else if first {
                        stretch(self.repeat)
                    } else {
                        TAP
                    };
                held.doubt = first;
                held.last = now;
                held.mods = mods;
                held.down = true;
                repeated
            }
            None => {
                self.keys.push(Held {
                    key,
                    mods,
                    last: now,
                    until: now + TAP,
                    down: true,
                    doubt: false,
                });
                false
            }
        }
    }

    /// The keys whose hold has ended by `now`.
    pub fn release(&mut self, now: Instant) -> Vec<Ended> {
        let mut ended = Vec::new();
        for held in &mut self.keys {
            if held.down && held.until <= now {
                if held.doubt {
                    held.doubt = false;
                    held.until = now + TAP;
                    ended.push(Ended::Tapped(held.key, held.mods));
                } else {
                    held.down = false;
                    ended.push(Ended::Released(held.key, held.mods));
                }
            }
        }
        let latest = self.first.1;
        self.keys
            .retain(|held| held.down || now.duration_since(held.last) <= latest);
        ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(parser: &mut Parser, bytes: &[u8]) -> Vec<KeyCode> {
        parser
            .feed(bytes, Instant::now())
            .iter()
            .map(|stroke| stroke.key)
            .collect()
    }

    #[test]
    fn characters_are_the_keys_that_type_them() {
        let mut parser = Parser::default();
        let strokes = parser.feed(b"aZ0) \r\t\x7f", Instant::now());
        let shift = KeyMods {
            shift: true,
            ..KeyMods::default()
        };
        assert_eq!(
            strokes,
            [
                Stroke {
                    key: KeyCode::A,
                    mods: KeyMods::default(),
                    character: Some('a')
                },
                Stroke {
                    key: KeyCode::Z,
                    mods: shift,
                    character: Some('Z')
                },
                Stroke {
                    key: KeyCode::Key0,
                    mods: KeyMods::default(),
                    character: Some('0')
                },
                Stroke {
                    key: KeyCode::Key0,
                    mods: shift,
                    character: Some(')')
                },
                Stroke {
                    key: KeyCode::Space,
                    mods: KeyMods::default(),
                    character: Some(' ')
                },
                key(KeyCode::Enter),
                key(KeyCode::Tab),
                key(KeyCode::Backspace),
            ]
        );
        // Every printable character has a key, and Control with a letter is that letter.
        for byte in 0x20..=0x7e {
            let stroke = parser.feed(&[byte], Instant::now());
            assert_eq!(stroke.len(), 1, "{byte}");
            assert_eq!(stroke[0].character, Some(byte as char));
            assert_ne!(stroke[0].key, KeyCode::Unknown);
        }
        let control = parser.feed(b"\x03\x12", Instant::now());
        assert_eq!(
            control.iter().map(|s| s.key).collect::<Vec<_>>(),
            [KeyCode::C, KeyCode::R]
        );
        assert!(control.iter().all(|s| s.mods.ctrl && s.character.is_none()));
        // A character of several bytes has no key; one that is cut waits for its rest.
        assert!(parser.feed(&"é".as_bytes()[..1], Instant::now()).is_empty());
        let rest = parser.feed(&"éa".as_bytes()[1..], Instant::now());
        assert_eq!(
            rest.iter()
                .map(|s| (s.key, s.character))
                .collect::<Vec<_>>(),
            [(KeyCode::Unknown, Some('é')), (KeyCode::A, Some('a'))]
        );
        assert!(parser.feed(&[0xff, 0x80], Instant::now()).is_empty());
        assert_eq!(keys(&mut parser, b"x"), [KeyCode::X]);
    }

    #[test]
    fn escape_sequences_are_the_keys_of_the_consoles_and_xterm() {
        use KeyCode::*;
        let mut parser = Parser::default();
        for (bytes, expected) in [
            // xterm and the FreeBSD console.
            (
                &b"\x1b[A\x1b[B\x1b[C\x1b[D\x1bOA\x1bOD"[..],
                vec![Up, Down, Right, Left, Up, Left],
            ),
            (
                b"\x1b[H\x1b[F\x1bOH\x1bOF\x1b[1~\x1b[4~\x1b[7~\x1b[8~",
                vec![Home, End, Home, End, Home, End, Home, End],
            ),
            (
                b"\x1b[2~\x1b[3~\x1b[5~\x1b[6~",
                vec![Insert, Delete, PageUp, PageDown],
            ),
            (
                b"\x1bOP\x1bOQ\x1bOR\x1bOS\x1b[15~\x1b[17~",
                vec![F1, F2, F3, F4, F5, F6],
            ),
            (
                b"\x1b[18~\x1b[19~\x1b[20~\x1b[21~\x1b[23~\x1b[24~",
                vec![F7, F8, F9, F10, F11, F12],
            ),
            (b"\x1b[11~\x1b[12~\x1b[13~\x1b[14~", vec![F1, F2, F3, F4]),
            // The Linux console.
            (
                b"\x1b[[A\x1b[[B\x1b[[C\x1b[[D\x1b[[E",
                vec![F1, F2, F3, F4, F5],
            ),
            // The old FreeBSD console.
            (
                b"\x1b[M\x1b[N\x1b[P\x1b[W\x1b[X\x1b[L\x1b[I\x1b[G",
                vec![F1, F2, F4, F11, F12, Insert, PageUp, PageDown],
            ),
        ] {
            assert_eq!(
                keys(&mut parser, bytes),
                expected,
                "{:?}",
                String::from_utf8_lossy(bytes)
            );
        }
        let held = parser.feed(b"\x1b[1;5C\x1b[1;2P\x1b[3;4~", Instant::now());
        assert_eq!(
            held.iter().map(|s| s.key).collect::<Vec<_>>(),
            [Right, F1, Delete]
        );
        assert_eq!(
            held.iter()
                .map(|s| (s.mods.shift, s.mods.alt, s.mods.ctrl))
                .collect::<Vec<_>>(),
            [
                (false, false, true),
                (true, false, false),
                (true, true, false)
            ]
        );
        // What is not a key is no key, and the next key is read.
        assert!(parser
            .feed(b"\x1b[99~\x1b[?25h\x1bOZ\x1b[[Z", Instant::now())
            .is_empty());
        // A sequence that is too long is forgotten at its 33rd byte.
        assert!(parser.feed(b"\x1b[1;", Instant::now()).is_empty());
        assert_eq!(keys(&mut parser, &[b'9'; 40]), [Key9; 11]);
        assert_eq!(keys(&mut parser, b"q\x1b[A"), [Q, Up]);
    }

    #[test]
    fn an_escape_character_alone_is_the_escape_key_after_a_while() {
        let now = Instant::now();
        let later = |milliseconds| now + Duration::from_millis(milliseconds);
        // A sequence that comes in pieces is one key, whenever its pieces come soon.
        for sequence in [&b"\x1b[A"[..], b"\x1bOP", b"\x1b[1;5C", b"\x1b[[A"] {
            for cut in 1..sequence.len() {
                let mut parser = Parser::default();
                assert!(parser.feed(&sequence[..cut], now).is_empty());
                let strokes = parser.feed(&sequence[cut..], later(20));
                assert_eq!(strokes.len(), 1, "{sequence:?} cut at {cut}");
                assert_ne!(strokes[0].key, KeyCode::Escape);
            }
        }
        let mut parser = Parser::default();
        assert!(parser.feed(b"\x1b", now).is_empty());
        assert!(parser.expire(later(49)).is_empty());
        assert_eq!(parser.expire(later(50)), [key(KeyCode::Escape)]);
        assert!(parser.expire(later(1000)).is_empty());
        // Two of them: the first is the key at once, the second waits like one alone.
        assert_eq!(parser.feed(b"\x1b\x1b", now), [key(KeyCode::Escape)]);
        assert_eq!(parser.expire(later(60)), [key(KeyCode::Escape)]);
        // One that has waited is the key before what comes next.
        assert!(parser.feed(b"\x1b", now).is_empty());
        assert_eq!(
            parser
                .feed(b"[A", later(70))
                .iter()
                .map(|stroke| stroke.key)
                .collect::<Vec<_>>(),
            [KeyCode::Escape, KeyCode::LeftBracket, KeyCode::A]
        );
        // Right before a character it is Alt with that key.
        let alt = parser.feed(b"\x1bx", now);
        assert_eq!(
            (alt.len(), alt[0].key, alt[0].mods.alt),
            (1, KeyCode::X, true)
        );
        // A sequence that never ends is forgotten.
        assert!(parser.feed(b"\x1b[1", now).is_empty());
        assert!(parser.expire(later(50)).is_empty());
        assert_eq!(keys(&mut parser, b"z"), [KeyCode::Z]);
    }

    #[test]
    fn a_tap_is_a_short_press_and_a_held_key_stays_down_while_it_repeats() {
        let start = Instant::now();
        let at = |milliseconds| start + Duration::from_millis(milliseconds);
        let none = KeyMods::default();
        let released = |key| [Ended::Released(key, none)];
        // The terminal waits 500 ms before it repeats a key and 30 ms between two repeats.
        let rate = Some((Duration::from_millis(500), Duration::from_millis(30)));
        let mut holds = Holds::new(rate);
        assert!(!holds.press(KeyCode::A, none, at(0)));
        assert!(holds.release(at(39)).is_empty());
        assert_eq!(holds.release(at(40)), released(KeyCode::A));
        assert!(holds.release(at(41)).is_empty());
        // The first repeat holds the key again without a new press, and then it stays down
        // from repeat to repeat: a key that is held is pressed once.
        assert!(holds.press(KeyCode::A, none, at(500)));
        assert!(holds.release(at(530)).is_empty());
        for repeat in 1..=10 {
            assert!(holds.press(KeyCode::A, none, at(500 + 30 * repeat)));
            assert!(holds.release(at(500 + 30 * repeat + 29)).is_empty());
        }
        // 65 ms after the last one, which is 30 * 1.5 + 20, the key is released.
        assert!(holds.release(at(864)).is_empty());
        assert_eq!(holds.release(at(865)), released(KeyCode::A));
        // A tap long after that is a tap again, and another key does not change it.
        assert!(!holds.press(KeyCode::A, none, at(3000)));
        assert!(!holds.press(KeyCode::D, none, at(3010)));
        assert_eq!(holds.release(at(3040)), released(KeyCode::A));
        assert_eq!(holds.release(at(3050)), released(KeyCode::D));
        // A second tap when the first repeat could come holds the key like one. No repeat
        // follows, so it was a tap: its press comes then, and it ends as a tap does.
        assert!(!holds.press(KeyCode::W, none, at(5000)));
        assert_eq!(holds.release(at(5040)), released(KeyCode::W));
        assert!(holds.press(KeyCode::W, none, at(5500)));
        assert!(holds.release(at(5564)).is_empty());
        assert_eq!(holds.release(at(5565)), [Ended::Tapped(KeyCode::W, none)]);
        assert!(holds.release(at(5604)).is_empty());
        assert_eq!(holds.release(at(5605)), released(KeyCode::W));
        // A second tap sooner than any repeat is a press at once, and so is one much later.
        assert!(!holds.press(KeyCode::W, none, at(5800)));
        assert_eq!(holds.release(at(5840)), released(KeyCode::W));
        assert!(!holds.press(KeyCode::W, none, at(6600)));
        assert_eq!(holds.release(at(6640)), released(KeyCode::W));
        // A key that repeats more slowly than the terminal said is held as long as it shows.
        assert!(!holds.press(KeyCode::W, none, at(8000)));
        assert_eq!(holds.release(at(8040)), released(KeyCode::W));
        assert!(holds.press(KeyCode::W, none, at(8500)));
        assert!(holds.press(KeyCode::W, none, at(8560)));
        assert!(holds.release(at(8669)).is_empty());
        assert_eq!(holds.release(at(8670)), released(KeyCode::W));
        // No hold is longer than 400 ms, and a terminal that tells nothing is taken for one
        // whose first repeat can come at any time, up to that of a slow one.
        let mut holds = Holds::new(None);
        assert!(!holds.press(KeyCode::Q, none, at(0)));
        assert_eq!(holds.release(at(40)), released(KeyCode::Q));
        assert!(holds.press(KeyCode::Q, none, at(250)));
        assert!(holds.press(KeyCode::Q, none, at(283)));
        assert!(holds.release(at(283 + 69)).is_empty());
        assert_eq!(holds.release(at(283 + 70)), released(KeyCode::Q));
        assert!(!holds.press(KeyCode::Q, none, at(2000)));
        assert_eq!(holds.release(at(2040)), released(KeyCode::Q));
        assert!(holds.press(KeyCode::Q, none, at(2900)));
        assert!(holds.release(at(2900 + 214)).is_empty());
        assert_eq!(
            holds.release(at(2900 + 215)),
            [Ended::Tapped(KeyCode::Q, none)]
        );
        assert_eq!(holds.release(at(2900 + 255)), released(KeyCode::Q));
        assert!(!holds.press(KeyCode::Q, none, at(4000)));
        assert!(holds.press(KeyCode::Q, none, at(4020)));
        assert!(holds.press(KeyCode::Q, none, at(4320)));
        assert!(holds.release(at(4719)).is_empty());
        assert_eq!(holds.release(at(4720)), released(KeyCode::Q));
    }
}
