//! Three-way merge of a crontab by line, against the fork-time base.

#[derive(Debug, PartialEq, Eq)]
pub enum Merge {
    /// Main's crontab already holds the fork's change.
    Unchanged,
    Merged(String),
    /// The fork edited or removed a line that main has changed too.
    Conflict(Vec<String>),
}

fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim_end().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Main's lines, without the lines the fork removed or edited, plus the lines the fork
/// added or edited. An edited line therefore appears once (its new form) and a removed
/// line is gone; lines only main has stay.
pub fn merge(base: &str, fork: &str, main: &str) -> Merge {
    let (base, fork, main) = (lines(base), lines(fork), lines(main));
    let removed: Vec<&String> = base.iter().filter(|line| !fork.contains(line)).collect();
    let added: Vec<&String> = fork.iter().filter(|line| !base.contains(line)).collect();
    let missing: Vec<String> = removed
        .iter()
        .filter(|line| !main.contains(line))
        .map(|line| (*line).clone())
        .collect();
    if !missing.is_empty() && !added.is_empty() && added.iter().any(|line| !main.contains(line)) {
        return Merge::Conflict(missing);
    }
    let mut out: Vec<String> = main
        .iter()
        .filter(|line| !removed.contains(line))
        .cloned()
        .collect();
    for line in added {
        if !out.contains(line) {
            out.push(line.clone());
        }
    }
    if out == main {
        return Merge::Unchanged;
    }
    Merge::Merged(format!("{}\n", out.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edited_line_appears_once_and_a_deleted_line_is_gone() {
        let base = "* * * * * a\n0 1 * * * b\n0 2 * * * c\n";
        let fork = "* * * * * a\n5 1 * * * b\n";
        let main = "* * * * * a\n0 1 * * * b\n0 2 * * * c\n0 3 * * * only-main\n";
        assert_eq!(
            merge(base, fork, main),
            Merge::Merged("* * * * * a\n0 3 * * * only-main\n5 1 * * * b\n".into())
        );
    }

    #[test]
    fn nothing_to_do_when_main_already_has_the_change() {
        let base = "a\n";
        assert_eq!(merge(base, "a\nb\n", "a\nb\n"), Merge::Unchanged);
        assert_eq!(merge("", "", "x\n"), Merge::Unchanged);
    }

    #[test]
    fn both_sides_editing_the_same_line_conflicts() {
        assert_eq!(
            merge("0 1 * * * b\n", "5 1 * * * b\n", "9 9 * * * b\n"),
            Merge::Conflict(vec!["0 1 * * * b".into()])
        );
    }

    #[test]
    fn a_line_both_sides_removed_is_not_a_conflict() {
        assert_eq!(merge("a\nb\n", "a\n", "a\n"), Merge::Unchanged);
    }
}
