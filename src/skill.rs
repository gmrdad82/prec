use crate::{Cli, Kind, app, parse_from, tape};
use clap::CommandFactory;
use clap::error::ErrorKind;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const BASE: &str = "app = \"alpha\"\nbin = \"alpha\"\ngrid = [80, 24]\n";

struct Doc {
    name: String,
    text: String,
}

struct Line {
    doc: String,
    no: usize,
    text: String,
}

impl Line {
    fn at(&self) -> String {
        format!("{}:{}", self.doc, self.no)
    }
}

struct Fence {
    at: String,
    lang: String,
    lines: Vec<Line>,
}

struct Parts {
    prose: Vec<Line>,
    fences: Vec<Fence>,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn docs() -> Vec<Doc> {
    let mut docs: Vec<Doc> = std::fs::read_dir(root().join("skills/prec"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| Doc {
            name: p.file_name().unwrap().to_string_lossy().into_owned(),
            text: std::fs::read_to_string(&p).unwrap(),
        })
        .collect();
    docs.sort_by(|a, b| a.name.cmp(&b.name));
    docs
}

fn parts(docs: &[Doc]) -> Parts {
    let mut parts = Parts {
        prose: Vec::new(),
        fences: Vec::new(),
    };
    for doc in docs {
        let mut open: Option<Fence> = None;
        for (i, raw) in doc.text.lines().enumerate() {
            let line = Line {
                doc: doc.name.clone(),
                no: i + 1,
                text: raw.to_string(),
            };
            if let Some(lang) = raw.trim_start().strip_prefix("```") {
                match open.take() {
                    Some(fence) => parts.fences.push(fence),
                    None => {
                        open = Some(Fence {
                            at: line.at(),
                            lang: lang.trim().to_string(),
                            lines: Vec::new(),
                        })
                    }
                }
            } else if let Some(fence) = open.as_mut() {
                fence.lines.push(line);
            } else {
                parts.prose.push(line);
            }
        }
        assert!(open.is_none(), "{}: a fence is never closed", doc.name);
    }
    parts
}

fn spans(line: &Line) -> Vec<String> {
    let pieces: Vec<&str> = line.text.split('`').collect();
    assert!(
        pieces.len() % 2 == 1,
        "{}: an odd number of backticks",
        line.at()
    );
    pieces
        .into_iter()
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

fn prec_words(text: &str) -> Option<Vec<String>> {
    let mut words = words(text);
    while words
        .first()
        .is_some_and(|w| w.contains('=') && !w.starts_with('-'))
    {
        words.remove(0);
    }
    if words.first().map(String::as_str) != Some("prec") {
        return None;
    }
    Some(
        words
            .into_iter()
            .map(|w| w.trim_matches(['[', ']']).to_string())
            .filter(|w| !w.is_empty())
            .collect(),
    )
}

fn commands(parts: &Parts) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for line in &parts.prose {
        for span in spans(line) {
            if let Some(args) = prec_words(&span) {
                out.push((line.at(), args));
            }
        }
    }
    for fence in parts.fences.iter().filter(|f| f.lang == "sh") {
        for line in fence
            .lines
            .iter()
            .filter(|l| !l.text.trim().is_empty() && !l.text.trim_start().starts_with('#'))
        {
            let args = prec_words(&line.text)
                .unwrap_or_else(|| panic!("{}: a sh line that is not prec", line.at()));
            out.push((line.at(), args));
        }
    }
    out
}

fn flags_in(text: &str) -> BTreeSet<String> {
    Regex::new(r"--[a-z][a-z-]*[a-z]")
        .unwrap()
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

fn help() -> (String, Vec<String>) {
    let mut cli = Cli::command();
    cli.build();
    let mut text = cli.render_long_help().to_string();
    let mut names = Vec::new();
    for sub in cli.get_subcommands_mut() {
        if sub.get_name() != "help" {
            names.push(sub.get_name().to_string());
        }
        text.push_str(&sub.render_long_help().to_string());
    }
    (text, names)
}

fn named(parts: &Parts) -> BTreeSet<String> {
    let mut all = String::new();
    for line in &parts.prose {
        all.push_str(&line.text);
        all.push('\n');
    }
    for fence in parts.fences.iter().filter(|f| f.lang == "sh") {
        for line in &fence.lines {
            all.push_str(&line.text);
            all.push('\n');
        }
    }
    flags_in(&all)
}

fn recipe(text: &str) -> String {
    if text.lines().any(|l| l.starts_with("app = ")) {
        text.to_string()
    } else {
        format!("{BASE}{text}")
    }
}

#[test]
fn the_skill_is_one_folder_of_linked_guides() {
    let docs = docs();
    let skill = docs.iter().find(|d| d.name == "SKILL.md").unwrap();
    assert!(
        skill.text.starts_with("---\nname: prec\ndescription: >\n"),
        "the front matter"
    );
    let link = Regex::new(r"\]\(([A-Za-z]+\.md)(#[a-z-]+)?\)").unwrap();
    let names: BTreeSet<&str> = docs.iter().map(|d| d.name.as_str()).collect();
    for doc in &docs {
        for c in link.captures_iter(&doc.text) {
            assert!(names.contains(&c[1]), "{}: links {}", doc.name, &c[1]);
        }
    }
    for doc in docs.iter().filter(|d| d.name != "SKILL.md") {
        assert!(
            skill.text.contains(&format!("]({})", doc.name)),
            "SKILL.md never links {}",
            doc.name
        );
    }
}

#[test]
fn every_command_the_skill_names_parses_and_asks_for_a_shot_the_demo_has() {
    let parts = parts(&docs());
    let list = commands(&parts);
    assert!(list.len() > 12, "{}", list.len());
    for (at, args) in &list {
        if args.get(1).is_some_and(|a| a.starts_with('<')) || args.len() == 1 {
            continue;
        }
        let kind = match parse_from(args) {
            Ok(kind) => kind,
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::DisplayHelp
                        | ErrorKind::DisplayVersion
                        | ErrorKind::MissingRequiredArgument
                ) =>
            {
                continue;
            }
            Err(e) => panic!("{at}: {args:?}: {e}"),
        };
        let (kind_name, shot, product, source) = match &kind {
            Kind::Clip(s) => ("clip", Some(&s.shot), Some(&s.product), Some(&s.source)),
            Kind::Still(s) => ("still", Some(&s.shot), Some(&s.product), Some(&s.source)),
            Kind::Record(s) => ("", Some(&s.shot), Some(&s.product), Some(&s.source)),
            Kind::Shots { product, source } => ("", None, Some(product), Some(source)),
            _ => ("", None, None, None),
        };
        let (Some(product), Some(source)) = (product, source) else {
            continue;
        };
        let Some(dir) = &source.app_dir else {
            continue;
        };
        let (terminal, _) = app::load(&root().join(dir)).unwrap_or_else(|e| panic!("{at}: {e}"));
        assert_eq!(&terminal.app, product, "{at}");
        if let Some(shot) = shot {
            let found = terminal
                .shots
                .get(shot)
                .unwrap_or_else(|| panic!("{at}: no shot {shot}"));
            assert!(
                kind_name.is_empty() || found.kind == kind_name,
                "{at}: {shot} is a {} shot",
                found.kind
            );
        }
    }
}

#[test]
fn every_cast_the_skill_renders_is_one_a_record_of_the_skill_writes() {
    let parts = parts(&docs());
    let mut written = BTreeSet::new();
    let mut read = Vec::new();
    for (at, args) in commands(&parts) {
        if args.len() < 2 || args[1].starts_with('<') {
            continue;
        }
        match parse_from(&args) {
            Ok(Kind::Record(s)) if s.source.app_dir.is_some() => {
                let dir = s.source.app_dir.clone().unwrap();
                let (terminal, _) = app::load(&root().join(dir)).unwrap();
                let kind = &terminal.shots[&s.shot].kind;
                written.insert(format!(
                    "tmp/renders/{0}/{kind}/{0}-{kind}-{1}.cast",
                    s.product, s.shot
                ));
            }
            Ok(Kind::Render(r)) if !r.cast.to_string_lossy().starts_with('<') => {
                read.push((at, r.cast.display().to_string()))
            }
            _ => {}
        }
    }
    assert!(!read.is_empty());
    for (at, cast) in read {
        assert!(written.contains(&cast), "{at}: {cast}");
    }
}

#[test]
fn every_flag_the_skill_names_is_in_the_help_and_every_help_flag_is_named() {
    let (help, _) = help();
    let in_help = flags_in(&help);
    let in_skill = named(&parts(&docs()));
    for flag in &in_skill {
        assert!(in_help.contains(flag), "{flag} is not in prec's help");
    }
    for flag in in_help
        .iter()
        .filter(|f| !["--help", "--version"].contains(&f.as_str()))
    {
        assert!(
            in_skill.contains(flag),
            "{flag} is in the help, not the skill"
        );
    }
}

#[test]
fn every_command_in_the_help_is_named_and_the_skill_names_no_other() {
    let (_, names) = help();
    let used: BTreeSet<String> = commands(&parts(&docs()))
        .into_iter()
        .filter_map(|(_, args)| args.get(1).cloned())
        .filter(|a| !a.starts_with('-') && !a.starts_with('<'))
        .collect();
    assert_eq!(used, names.into_iter().collect::<BTreeSet<_>>());
}

#[test]
fn every_recipe_in_the_skill_loads_as_prec_loads_one() {
    let parts = parts(&docs());
    let toml: Vec<&Fence> = parts.fences.iter().filter(|f| f.lang == "toml").collect();
    assert!(toml.len() >= 3, "{}", toml.len());
    for fence in toml {
        let text: Vec<&str> = fence.lines.iter().map(|l| l.text.trim_start()).collect();
        let t = app::parse(&recipe(&text.join("\n")), Path::new("terminal.toml"))
            .unwrap_or_else(|e| panic!("{}: {e}", fence.at));
        assert!(!t.app.is_empty());
    }
}

#[test]
fn the_first_recipe_is_the_demos_with_the_same_shots_and_tapes() {
    let parts = parts(&docs());
    let fence = parts
        .fences
        .iter()
        .find(|f| f.lang == "toml" && f.at.starts_with("recipe.md"))
        .unwrap();
    let text: Vec<&str> = fence.lines.iter().map(|l| l.text.as_str()).collect();
    let shown = app::parse(&text.join("\n"), Path::new("terminal.toml")).unwrap();
    let demo = root().join("examples/demo");
    let (real, _) = app::load(&demo).unwrap();
    assert_eq!(shown.app, real.app);
    assert_eq!(shown.grid, real.grid);
    assert_eq!(shown.build.unwrap().bin_dir, real.build.unwrap().bin_dir);
    let tapes: BTreeMap<_, _> = real.shots.iter().map(|(n, s)| (n, &s.tape)).collect();
    for (name, shot) in &shown.shots {
        assert_eq!(tapes.get(name), Some(&&shot.tape), "{name}");
        assert!(demo.join(&shot.tape).is_file(), "{}", shot.tape);
    }
}

#[test]
fn every_tape_in_the_skill_parses_as_prec_reads_one() {
    let parts = parts(&docs());
    let tapes: Vec<&Fence> = parts
        .fences
        .iter()
        .filter(|f| f.lang == "text" && !f.lines[0].text.starts_with("prec:"))
        .collect();
    assert!(tapes.len() >= 2, "{}", tapes.len());
    for fence in tapes {
        for line in &fence.lines {
            tape::parse_line(&line.text).unwrap_or_else(|e| panic!("{}: {e}", line.at()));
        }
    }
}

#[test]
fn the_demo_tapes_the_skill_shows_are_the_demos_own() {
    let parts = parts(&docs());
    let shown = parts
        .fences
        .iter()
        .find(|f| f.lang == "text" && f.at.starts_with("tapes.md"))
        .unwrap();
    let text: Vec<&str> = shown.lines.iter().map(|l| l.text.as_str()).collect();
    let real =
        std::fs::read_to_string(root().join("examples/demo/render/terminal/typing.tape")).unwrap();
    assert_eq!(text.join("\n"), real.trim_end());
}

#[test]
fn every_variable_and_path_the_skill_names_exists() {
    let docs = docs();
    let mut sources = String::new();
    for entry in std::fs::read_dir(root().join("src")).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|n| n != "skill.rs") {
            sources.push_str(&std::fs::read_to_string(&path).unwrap());
        }
    }
    let var = Regex::new(r"PREC_[A-Z]+").unwrap();
    let path = Regex::new(r"examples/[A-Za-z0-9_./-]*[A-Za-z0-9_]").unwrap();
    let mut vars = BTreeSet::new();
    for doc in &docs {
        for m in var.find_iter(&doc.text) {
            assert!(
                sources.contains(&format!("\"{}\"", m.as_str())),
                "{}: {} is no variable prec reads",
                doc.name,
                m.as_str()
            );
            vars.insert(m.as_str().to_string());
        }
        for m in path.find_iter(&doc.text) {
            assert!(
                root().join(m.as_str()).exists(),
                "{}: {}",
                doc.name,
                m.as_str()
            );
        }
    }
    assert!(vars.contains("PREC_AUDIO"));
}
