//! "did you mean ...?" suggestions and hints for habits carried over from other languages.

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

/// `to_number` → `toNumber`
pub fn to_camel(s: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in s.chars() {
        if c == '_' && !out.is_empty() {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The closest candidate to `name`, if it is close enough to be a plausible typo.
pub fn closest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = match name.chars().count() {
        0..=2 => 1,
        3..=5 => 2,
        _ => 3,
    };
    let lower = name.to_lowercase();
    let camel = to_camel(name);
    let mut best: Option<(usize, &'a str)> = None;
    for c in candidates {
        if c == name {
            continue;
        }
        let d = if c == camel || c.to_lowercase() == lower { 0 } else { edit_distance(name, c) };
        if d <= limit && best.is_none_or(|(bd, bc)| d < bd || (d == bd && c < bc)) {
            best = Some((d, c));
        }
    }
    best.map(|(_, c)| c)
}

/// `did you mean "x"?` for the closest candidate.
pub fn did_you_mean<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    closest(name, candidates).map(|c| {
        if c == to_camel(name) && name.contains('_') {
            format!("did you mean \"{c}\"? LiPi names use camelCase.")
        } else {
            format!("did you mean \"{c}\"?")
        }
    })
}

/// Hints for names that come from other languages.
pub fn foreign_name_hint(name: &str) -> Option<&'static str> {
    Some(match name {
        "print" | "println" | "console" | "puts" | "echo" | "printf" => "To print something in LiPi, write: show \"Hello\"",
        "nil" | "None" | "undefined" | "NULL" | "Null" => "LiPi calls the empty value null.",
        "True" | "TRUE" => "Booleans in LiPi are lowercase: true",
        "False" | "FALSE" => "Booleans in LiPi are lowercase: false",
        "let" | "var" => "LiPi doesn't need let/var. Just write: name = value",
        "def" | "fn" | "func" => {
            "Define a function with `function add(a, b)` or just `add(a, b)`, followed by an indented body."
        }
        "elif" | "elsif" | "elseif" => "Write `else if` in LiPi.",
        "len" | "size" => "Use the .length property, for example: items.length",
        "str" => "Use toString(value) to convert something to text.",
        "int" | "float" | "parseInt" | "parseFloat" => "Use toNumber(value), toInteger(value) or toDecimal(value).",
        "this" => "Inside a type's methods, LiPi calls the current object self.",
        "import" | "require" | "include" => "LiPi loads modules with use, for example: use math",
        _ => return None,
    })
}
