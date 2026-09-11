//! "did you mean ...?" suggestions and hints for habits carried over from other languages.

/// Edit distance used to suggest similarly spelled names. Swapping two
/// neighbouring letters counts as one edit, since it's the most common typo
/// ("nmae" is 1 away from "name").
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swapped_letters_are_one_edit() {
        assert_eq!(edit_distance("nmae", "name"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(closest("nmae", ["image", "name", "age"]), Some("name"));
        // Equal distance: the name with the same first letter wins over alphabetical order.
        assert_eq!(closest("nmae", ["image", "naam"]), Some("naam"));
    }
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
    let first = name.chars().next();
    let len = name.chars().count();
    // Closest first; ties go to a name with the same first letter, then the
    // one nearest in length, then alphabetical ("nmae" suggests "naam", not "image").
    let rank = |c: &str, d: usize| (d, c.chars().next() != first, c.chars().count().abs_diff(len), c.to_string());
    let mut best: Option<((usize, bool, usize, String), &'a str)> = None;
    for c in candidates {
        if c == name {
            continue;
        }
        let d = if c == camel || c.to_lowercase() == lower { 0 } else { edit_distance(name, c) };
        if d > limit {
            continue;
        }
        let r = rank(c, d);
        if best.as_ref().is_none_or(|(br, _)| r < *br) {
            best = Some((r, c));
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
