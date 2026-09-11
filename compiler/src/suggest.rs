//! "Did you mean ...?" suggestions and hints for habits carried over from other languages.

/// Levenshtein edit distance, used to suggest similarly spelled names.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The closest candidate to `name`, if it is close enough to be a plausible typo.
pub fn closest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = match name.chars().count() {
        0..=2 => 1,
        3..=5 => 2,
        _ => 3,
    };
    let lower = name.to_lowercase();
    candidates
        .into_iter()
        .filter(|c| *c != name)
        .map(|c| {
            let d = if c.to_lowercase() == lower { 0 } else { edit_distance(name, c) };
            (d, c)
        })
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

/// Hints for names that come from other languages.
pub fn foreign_name_hint(name: &str) -> Option<&'static str> {
    Some(match name {
        "print" | "println" | "console" | "puts" | "echo" | "printf" => {
            "To print something in Lipi, write: show \"Hello\""
        }
        "null" | "None" | "undefined" | "NULL" => "Lipi calls the empty value `nil`.",
        "True" | "TRUE" => "Booleans in Lipi are lowercase: true",
        "False" | "FALSE" => "Booleans in Lipi are lowercase: false",
        "let" | "var" | "const_" => "Lipi doesn't need let/var. Just write: name = value",
        "function" | "def" | "fn" | "func" => {
            "Define a function by writing its name and parameters, then an indented body:\n    add(a, b)\n        return a + b"
        }
        "elif" | "elsif" | "elseif" => "Write `else if` in Lipi.",
        "len" | "length" | "size" => "Use the .length property, for example: items.length",
        "str" | "String" => "Use to_string(value) to convert something to text.",
        "int" | "float" | "parseInt" | "parseFloat" | "Number" => {
            "Use to_number(value) to convert text to a number."
        }
        "this" => "Inside a type's methods, Lipi calls the current object `self`.",
        _ => return None,
    })
}
