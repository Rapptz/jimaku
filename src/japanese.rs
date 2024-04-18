use std::{borrow::Cow, ops::RangeInclusive};

use crate::borrowed::MaybeBorrowedString;

const fn is_diacritic(c: char) -> bool {
    c == 'ā' || c == 'ī' || c == 'ū' || c == 'ē' || c == 'ō' || c == 'â' || c == 'î' || c == 'û' || c == 'ê' || c == 'ô'
}

/// Transforms a romaji string with macrons or circumflexes to ones without it.
pub fn normalize_diacritics(s: &str) -> Cow<'_, str> {
    if s.is_ascii() {
        return s.into();
    }

    // Slower path is to walk through it byte by byte
    match s.find(is_diacritic) {
        Some(index) => {
            let mut output = String::with_capacity(s.len());
            output.push_str(&s[0..index]);
            for c in s[index..].chars() {
                match c {
                    'ā' | 'â' => output.push_str("aa"),
                    'ī' | 'î' => output.push_str("ii"),
                    'ū' | 'û' => output.push_str("uu"),
                    'ē' | 'ê' => output.push_str("ee"),
                    'ō' | 'ô' => output.push_str("ou"),
                    'Ā' | 'Â' => output.push_str("Aa"),
                    'Ī' | 'Î' => output.push_str("Ii"),
                    'Ū' | 'Û' => output.push_str("Uu"),
                    'Ē' | 'Ê' => output.push_str("Ee"),
                    'Ō' | 'Ô' => output.push_str("Ou"),
                    _ => output.push(c),
                }
            }
            Cow::Owned(output)
        }
        None => s.into(),
    }
}

/// Deserializes the string by replacing diacritics with the ASCII counterpart
pub fn normalized_ascii_representation<'de, D>(de: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let borrowed = MaybeBorrowedString::deserialize(de)?;
    Ok(normalize_diacritics(borrowed.as_str()).into_owned())
}

#[inline]
const fn is_yoon_char(ch: &char) -> bool {
    *ch == 'a' || *ch == 'u' || *ch == 'o'
}

#[inline]
const fn is_vowel(ch: &char) -> bool {
    *ch == 'a' || *ch == 'e' || *ch == 'i' || *ch == 'o' || *ch == 'u'
}

/// Returns `true` if a character is a kanji, hiragana, or katakana character
pub fn is_japanese_char(ch: char) -> bool {
    const CJK_MAPPING: [RangeInclusive<char>; 3] = [
        '\u{3040}'..='\u{30ff}', // Hiragana + Katakana
        '\u{ff66}'..='\u{ff9d}', // Half-width Katakana
        '\u{4e00}'..='\u{9faf}', // Common + Uncommon Kanji
    ];
    CJK_MAPPING.iter().any(|c| c.contains(&ch))
}

/// Converts the romaji text to hiragana, in a lossy manner.
///
/// If the character doesn't map to a hiragana character then it's kept mostly as-is.
///
/// Note that this is loose-ly based off of Hepburn romanization. This doesn't
/// support most things from Nihon/Kunrei shiki.
pub fn romaji_to_hiragana(s: &str) -> String {
    let mut output = String::with_capacity(s.len());
    let mut parser = s.chars().map(|c| c.to_ascii_lowercase()).peekable();
    while let Some(ch) = parser.next() {
        // a e i o u
        // on their own is usually fine
        match ch {
            'a' => output.push('あ'),
            'e' => output.push('え'),
            'i' => output.push('い'),
            'o' => output.push('お'),
            'u' => output.push('う'),
            'k' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('か');
                            parser.next();
                        }
                        'i' => {
                            output.push('き');
                            parser.next();
                        }
                        'u' => {
                            output.push('く');
                            parser.next();
                        }
                        'e' => {
                            output.push('け');
                            parser.next();
                        }
                        'o' => {
                            output.push('こ');
                            parser.next();
                        }
                        'y' => {
                            parser.next();
                            if let Some(yoon) = parser.next_if(is_yoon_char) {
                                match yoon {
                                    'a' => output.push_str("きゃ"),
                                    'o' => output.push_str("きょ"),
                                    'u' => output.push_str("きゅ"),
                                    _ => {}
                                }
                            } else {
                                output.push(ch);
                            }
                        }
                        'k' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            's' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('さ');
                            parser.next();
                        }
                        'u' => {
                            output.push('す');
                            parser.next();
                        }
                        'e' => {
                            output.push('せ');
                            parser.next();
                        }
                        'o' => {
                            output.push('そ');
                            parser.next();
                        }
                        'h' => {
                            parser.next();
                            if let Some(yoon) = parser.next_if(|c| is_yoon_char(c) || *c == 'i') {
                                match yoon {
                                    'a' => output.push_str("しゃ"),
                                    'o' => output.push_str("しょ"),
                                    'u' => output.push_str("しゅ"),
                                    'i' => output.push('し'),
                                    _ => {
                                        unreachable!()
                                    }
                                }
                            } else {
                                output.push(ch);
                            }
                        }
                        's' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            'c' => {
                if parser.next_if_eq(&'h').is_some() {
                    if let Some(yoon) = parser.next_if(|c| is_yoon_char(c) || *c == 'i') {
                        match yoon {
                            'a' => output.push_str("ちゃ"),
                            'o' => output.push_str("ちょ"),
                            'u' => output.push_str("ちゅ"),
                            'i' => output.push('ち'),
                            _ => {
                                unreachable!()
                            }
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            't' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('た');
                            parser.next();
                        }
                        'e' => {
                            output.push('て');
                            parser.next();
                        }
                        'o' => {
                            output.push('と');
                            parser.next();
                        }
                        's' => {
                            parser.next();
                            if parser.next_if_eq(&'u').is_some() {
                                output.push('つ');
                            } else {
                                output.push(ch);
                                output.push('s');
                            }
                        }
                        't' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            'n' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('な');
                            parser.next();
                        }
                        'i' => {
                            output.push('に');
                            parser.next();
                        }
                        'u' => {
                            output.push('ぬ');
                            parser.next();
                        }
                        'e' => {
                            output.push('ね');
                            parser.next();
                        }
                        'o' => {
                            output.push('の');
                            parser.next();
                        }
                        'y' => {
                            parser.next();
                            if let Some(yoon) = parser.next_if(is_yoon_char) {
                                match yoon {
                                    'a' => output.push_str("にゃ"),
                                    'o' => output.push_str("にょ"),
                                    'u' => output.push_str("にゅ"),
                                    _ => {
                                        unreachable!()
                                    }
                                }
                            } else {
                                output.push(ch);
                            }
                        }
                        // n is rather special
                        // hentai => he n ta i
                        // annai => a n na i
                        // onna => o n na
                        // senpai => se n pa i
                        // kan'i => ka n' i
                        // kin'emon => ki n' e mo n
                        // Essentially before a consonant it becomes an N
                        // but before a vowel it requires a '
                        '\'' => {
                            output.push('ん');
                            parser.next();
                        }
                        _ if !is_vowel(p) => {
                            output.push('ん');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push('ん');
                }
            }
            'h' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('は');
                            parser.next();
                        }
                        'i' => {
                            output.push('ひ');
                            parser.next();
                        }
                        'u' => {
                            output.push('ふ');
                            parser.next();
                        }
                        'e' => {
                            output.push('へ');
                            parser.next();
                        }
                        'o' => {
                            output.push('ほ');
                            parser.next();
                        }
                        'y' => {
                            parser.next();
                            if let Some(yoon) = parser.next_if(is_yoon_char) {
                                match yoon {
                                    'a' => output.push_str("ひゃ"),
                                    'o' => output.push_str("ひょ"),
                                    'u' => output.push_str("ひゅ"),
                                    _ => {
                                        unreachable!()
                                    }
                                }
                            } else {
                                output.push(ch);
                            }
                        }
                        'h' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            'f' => {
                if parser.next_if_eq(&'u').is_some() {
                    output.push('ふ');
                } else {
                    output.push('f');
                }
            }
            'm' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('ま');
                            parser.next();
                        }
                        'i' => {
                            output.push('み');
                            parser.next();
                        }
                        'u' => {
                            output.push('む');
                            parser.next();
                        }
                        'e' => {
                            output.push('め');
                            parser.next();
                        }
                        'o' => {
                            output.push('も');
                            parser.next();
                        }
                        'y' => {
                            parser.next();
                            if let Some(yoon) = parser.next_if(is_yoon_char) {
                                match yoon {
                                    'a' => output.push_str("みゃ"),
                                    'o' => output.push_str("みょ"),
                                    'u' => output.push_str("みゅ"),
                                    _ => {
                                        unreachable!()
                                    }
                                }
                            } else {
                                output.push(ch);
                            }
                        }
                        'm' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push('ん');
                        }
                    }
                } else {
                    output.push('ん');
                }
            }
            'y' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('や');
                            parser.next();
                        }
                        'u' => {
                            output.push('ゆ');
                            parser.next();
                        }
                        'o' => {
                            output.push('よ');
                            parser.next();
                        }
                        'y' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            'r' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('ら');
                            parser.next();
                        }
                        'i' => {
                            output.push('り');
                            parser.next();
                        }
                        'u' => {
                            output.push('る');
                            parser.next();
                        }
                        'e' => {
                            output.push('れ');
                            parser.next();
                        }
                        'o' => {
                            output.push('ろ');
                            parser.next();
                        }
                        'y' => {
                            parser.next();
                            if let Some(yoon) = parser.next_if(is_yoon_char) {
                                match yoon {
                                    'a' => output.push_str("りゃ"),
                                    'o' => output.push_str("りょ"),
                                    'u' => output.push_str("りゅ"),
                                    _ => {
                                        unreachable!()
                                    }
                                }
                            } else {
                                output.push(ch);
                            }
                        }
                        'r' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            'w' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('わ');
                            parser.next();
                        }
                        'o' => {
                            output.push('を');
                            parser.next();
                        }
                        _ => output.push('w'),
                    }
                } else {
                    output.push(ch);
                }
            }
            'g' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('が');
                            parser.next();
                        }
                        'i' => {
                            output.push('ぎ');
                            parser.next();
                        }
                        'u' => {
                            output.push('ぐ');
                            parser.next();
                        }
                        'e' => {
                            output.push('げ');
                            parser.next();
                        }
                        'o' => {
                            output.push('ご');
                            parser.next();
                        }
                        'y' => {
                            parser.next();
                            if let Some(yoon) = parser.next_if(is_yoon_char) {
                                match yoon {
                                    'a' => output.push_str("ぎゃ"),
                                    'o' => output.push_str("ぎょ"),
                                    'u' => output.push_str("ぎゅ"),
                                    _ => {
                                        unreachable!()
                                    }
                                }
                            } else {
                                output.push(ch);
                            }
                        }
                        'g' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            'j' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'i' => {
                            output.push('じ');
                            parser.next();
                        }
                        'a' => {
                            output.push_str("じゃ");
                            parser.next();
                        }
                        'o' => {
                            output.push_str("じょ");
                            parser.next();
                        }
                        'u' => {
                            output.push_str("じゅ");
                            parser.next();
                        }
                        'j' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            'z' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('ざ');
                            parser.next();
                        }
                        'o' => {
                            output.push('ぞ');
                            parser.next();
                        }
                        'u' => {
                            output.push('ず');
                            parser.next();
                        }
                        'e' => {
                            output.push('ぜ');
                            parser.next();
                        }
                        'z' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            'd' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('だ');
                            parser.next();
                        }
                        'o' => {
                            output.push('ど');
                            parser.next();
                        }
                        'u' => {
                            output.push('づ');
                            parser.next();
                        }
                        'e' => {
                            output.push('で');
                            parser.next();
                        }
                        'd' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            'b' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('ば');
                            parser.next();
                        }
                        'i' => {
                            output.push('び');
                            parser.next();
                        }
                        'u' => {
                            output.push('ぶ');
                            parser.next();
                        }
                        'e' => {
                            output.push('べ');
                            parser.next();
                        }
                        'o' => {
                            output.push('ぼ');
                            parser.next();
                        }
                        'y' => {
                            parser.next();
                            if let Some(yoon) = parser.next_if(is_yoon_char) {
                                match yoon {
                                    'a' => output.push_str("びゃ"),
                                    'o' => output.push_str("びょ"),
                                    'u' => output.push_str("びゅ"),
                                    _ => {
                                        unreachable!()
                                    }
                                }
                            } else {
                                output.push(ch);
                            }
                        }
                        'b' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            'p' => {
                if let Some(p) = parser.peek() {
                    match p {
                        'a' => {
                            output.push('ぱ');
                            parser.next();
                        }
                        'i' => {
                            output.push('ぴ');
                            parser.next();
                        }
                        'u' => {
                            output.push('ぷ');
                            parser.next();
                        }
                        'e' => {
                            output.push('ぺ');
                            parser.next();
                        }
                        'o' => {
                            output.push('ぽ');
                            parser.next();
                        }
                        'y' => {
                            parser.next();
                            if let Some(yoon) = parser.next_if(is_yoon_char) {
                                match yoon {
                                    'a' => output.push_str("ぴゃ"),
                                    'o' => output.push_str("ぴょ"),
                                    'u' => output.push_str("ぴゅ"),
                                    _ => {
                                        unreachable!()
                                    }
                                }
                            } else {
                                output.push(ch);
                            }
                        }
                        'p' => {
                            output.push('っ');
                        }
                        _ => {
                            output.push(ch);
                        }
                    }
                } else {
                    output.push(ch);
                }
            }
            _ => {}
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalized_diacritics() {
        assert_eq!(normalize_diacritics("mūsu"), "muusu");
        assert_eq!(normalize_diacritics("shanpū"), "shanpuu");
        assert_eq!(normalize_diacritics("pūru"), "puuru");
        assert_eq!(normalize_diacritics("rādo"), "raado");
        assert_eq!(normalize_diacritics("Tendō"), "Tendou");
        assert_eq!(normalize_diacritics("Ryōga"), "Ryouga");
        assert_eq!(normalize_diacritics("onēchan"), "oneechan");
        assert_eq!(normalize_diacritics("obāsan"), "obaasan");
    }

    #[test]
    fn test_romaji_to_hiragana() {
        assert_eq!(romaji_to_hiragana("kakkoii"), "かっこいい");
        assert_eq!(romaji_to_hiragana("onna"), "おんな");
        assert_eq!(romaji_to_hiragana("senpai"), "せんぱい");
        assert_eq!(romaji_to_hiragana("sempai"), "せんぱい");
        assert_eq!(romaji_to_hiragana("hentai"), "へんたい");
        assert_eq!(romaji_to_hiragana("annai"), "あんない");
        assert_eq!(romaji_to_hiragana("taihen"), "たいへん");
        assert_eq!(romaji_to_hiragana("kan'i"), "かんい");
        assert_eq!(romaji_to_hiragana("kin'emon"), "きんえもん");
        assert_eq!(romaji_to_hiragana("sekkyokuteki"), "せっきょくてき");
        assert_eq!(romaji_to_hiragana("jiyuu"), "じゆう");
    }
}
