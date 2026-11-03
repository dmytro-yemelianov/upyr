//! Text transforms applied to a selection: Ukrainian → Latin transliteration
//! and case cycling. They change only the text, never the keyboard layout.

/// A transform for the selected text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTransform {
    /// Ukrainian → Latin by the official national system (Resolution of the
    /// Cabinet of Ministers of Ukraine No. 55, 2010). The system is not
    /// reversible, so there is no Latin → Ukrainian direction.
    Transliterate,
    /// lower case → UPPER CASE → Title Case → lower case.
    CycleCase,
}

impl TextTransform {
    pub fn apply(self, text: &str) -> String {
        match self {
            Self::Transliterate => transliterate(text),
            Self::CycleCase => cycle_case(text),
        }
    }
}

/// Ukrainian → Latin by Resolution No. 55 (2010). Other characters pass
/// through unchanged.
pub fn transliterate(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let in_word = |index: usize| {
        chars
            .get(index)
            .is_some_and(|&c| c.is_alphabetic() || is_apostrophe(c))
    };
    let mut latin = String::with_capacity(text.len());
    for (index, &character) in chars.iter().enumerate() {
        let lower = character.to_lowercase().next().unwrap_or(character);
        let previous = index.checked_sub(1).map(|before| chars[before]);
        // The apostrophe inside a Ukrainian word is not transliterated.
        if is_apostrophe(character)
            && previous.is_some_and(is_ukrainian)
            && chars.get(index + 1).copied().is_some_and(is_ukrainian)
        {
            continue;
        }
        let word_start = index == 0 || !in_word(index - 1);
        let after_z = previous.is_some_and(|c| matches!(c, 'з' | 'З'));
        let romanized = match lower {
            'а' => "a",
            'б' => "b",
            'в' => "v",
            'г' if after_z => "gh",
            'г' => "h",
            'ґ' => "g",
            'д' => "d",
            'е' => "e",
            'є' if word_start => "ye",
            'є' => "ie",
            'ж' => "zh",
            'з' => "z",
            'и' => "y",
            'і' => "i",
            'ї' if word_start => "yi",
            'ї' => "i",
            'й' if word_start => "y",
            'й' => "i",
            'к' => "k",
            'л' => "l",
            'м' => "m",
            'н' => "n",
            'о' => "o",
            'п' => "p",
            'р' => "r",
            'с' => "s",
            'т' => "t",
            'у' => "u",
            'ф' => "f",
            'х' => "kh",
            'ц' => "ts",
            'ч' => "ch",
            'ш' => "sh",
            'щ' => "shch",
            'ь' => "",
            'ю' if word_start => "yu",
            'ю' => "iu",
            'я' if word_start => "ya",
            'я' => "ia",
            _ => {
                latin.push(character);
                continue;
            }
        };
        if character.is_uppercase() {
            if word_is_all_caps(&chars, index) {
                latin.push_str(&romanized.to_uppercase());
            } else {
                let mut letters = romanized.chars();
                if let Some(first) = letters.next() {
                    latin.extend(first.to_uppercase());
                    latin.extend(letters);
                }
            }
        } else {
            latin.push_str(romanized);
        }
    }
    latin
}

/// lower → UPPER → Title → lower, judged on the selection's letters.
pub fn cycle_case(text: &str) -> String {
    let mut letters = text.chars().filter(|c| c.is_alphabetic()).peekable();
    if letters.peek().is_none() {
        return text.to_owned();
    }
    let letters: Vec<char> = letters.collect();
    if letters.iter().all(|c| !c.is_uppercase()) {
        text.to_uppercase()
    } else if letters.iter().all(|c| !c.is_lowercase()) {
        title_case(text)
    } else {
        text.to_lowercase()
    }
}

fn title_case(text: &str) -> String {
    let mut titled = String::with_capacity(text.len());
    let mut word_start = true;
    for character in text.chars() {
        if character.is_alphabetic() {
            if word_start {
                titled.extend(character.to_uppercase());
            } else {
                titled.extend(character.to_lowercase());
            }
            word_start = false;
        } else {
            titled.push(character);
            // `п'ять` stays one word.
            word_start = !is_apostrophe(character);
        }
    }
    titled
}

fn is_apostrophe(character: char) -> bool {
    matches!(character, '\'' | '’' | 'ʼ')
}

fn is_ukrainian(character: char) -> bool {
    ('\u{0400}'..='\u{04FF}').contains(&character)
}

/// A word of two or more letters, all upper case, keeps its digraphs upper
/// case too (`ЖУК` → `ZHUK`, but `Жук` → `Zhuk`).
fn word_is_all_caps(chars: &[char], index: usize) -> bool {
    let in_word = |c: &char| c.is_alphabetic() || is_apostrophe(*c);
    let start = chars[..index]
        .iter()
        .rposition(|c| !in_word(c))
        .map_or(0, |position| position + 1);
    let end = chars[index..]
        .iter()
        .position(|c| !in_word(c))
        .map_or(chars.len(), |position| index + position);
    let letters: Vec<&char> = chars[start..end]
        .iter()
        .filter(|c| c.is_alphabetic())
        .collect();
    letters.len() >= 2 && letters.iter().all(|c| !c.is_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transliterates_the_official_examples() {
        // Every example from the resolution's table.
        for (ukrainian, latin) in [
            ("Алушта", "Alushta"),
            ("Андрій", "Andrii"),
            ("Борщагівка", "Borshchahivka"),
            ("Вінниця", "Vinnytsia"),
            ("Гадяч", "Hadiach"),
            ("Згорани", "Zghorany"),
            ("Ґалаґан", "Galagan"),
            ("Дон", "Don"),
            ("Рівне", "Rivne"),
            ("Єнакієве", "Yenakiieve"),
            ("Гаєвич", "Haievych"),
            ("Житомир", "Zhytomyr"),
            ("Закарпаття", "Zakarpattia"),
            ("Медвин", "Medvyn"),
            ("Іршава", "Irshava"),
            ("Їжакевич", "Yizhakevych"),
            ("Кадиївка", "Kadyivka"),
            ("Йосипівка", "Yosypivka"),
            ("Стрий", "Stryi"),
            ("Київ", "Kyiv"),
            ("Лебедин", "Lebedyn"),
            ("Миколаїв", "Mykolaiv"),
            ("Ніжин", "Nizhyn"),
            ("Одеса", "Odesa"),
            ("Полтава", "Poltava"),
            ("Решетилівка", "Reshetylivka"),
            ("Суми", "Sumy"),
            ("Тернопіль", "Ternopil"),
            ("Ужгород", "Uzhhorod"),
            ("Фастів", "Fastiv"),
            ("Харків", "Kharkiv"),
            ("Біла Церква", "Bila Tserkva"),
            ("Чернівці", "Chernivtsi"),
            ("Шостка", "Shostka"),
            ("Гоща", "Hoshcha"),
            ("Юрій", "Yurii"),
            ("Крюківка", "Kriukivka"),
            ("Яготин", "Yahotyn"),
            ("Ічня", "Ichnia"),
            ("Знам'янка", "Znamianka"),
            ("Знамʼянка", "Znamianka"),
        ] {
            assert_eq!(transliterate(ukrainian), latin, "{ukrainian}");
        }
    }

    #[test]
    fn transliteration_keeps_all_caps_and_other_text() {
        assert_eq!(transliterate("ЖИТОМИР"), "ZHYTOMYR");
        assert_eq!(transliterate("ЩАСТЯ"), "SHCHASTIA");
        assert_eq!(transliterate("місто Львів, 2026!"), "misto Lviv, 2026!");
        assert_eq!(transliterate("Hello"), "Hello");
    }

    #[test]
    fn case_cycles_lower_upper_title() {
        assert_eq!(cycle_case("привіт світ"), "ПРИВІТ СВІТ");
        assert_eq!(cycle_case("ПРИВІТ СВІТ"), "Привіт Світ");
        assert_eq!(cycle_case("Привіт Світ"), "привіт світ");
        assert_eq!(cycle_case("hello, WORLD"), "hello, world");
        assert_eq!(cycle_case("42 — ok"), "42 — OK");
    }
}
