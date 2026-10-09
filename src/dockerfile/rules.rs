//! hadolint 2.15.1's DL rules, one function each, in code order. The
//! messages are hadolint's, word for word. Rules hadolint applies to the
//! instruction inside `ONBUILD` as well (`rule <> onbuild rule`) run a
//! second time over those, with their own state.

use std::collections::{BTreeMap, BTreeSet};

use super::Failure;
use super::config::{Config, LabelType, Severity};
use super::parser::{
    Args, ArgsKind, BaseImage, Image, Instruction, InstructionPos, Pairs, Port, PortSpec, RunFlags,
    RunMount,
};
use super::shell::{Command, ParsedShell, contains_seq};

type Doc<'a> = [&'a InstructionPos];

/// `dropQuotes`: surrounding single or double quotes removed.
pub fn drop_quotes(s: &str) -> &str {
    s.trim_matches(['"', '\''])
}

const ARCHIVE_EXTENSIONS: &[&str] = &[
    ".tar", ".Z", ".bz2", ".gz", ".lz", ".lzma", ".tZ", ".tb2", ".tbz", ".tbz2", ".tgz", ".tlz",
    ".tpz", ".txz", ".xz",
];

fn is_archive(path: &str) -> bool {
    let p = drop_quotes(path);
    ARCHIVE_EXTENSIONS.iter().any(|e| p.ends_with(e))
}

fn failure(code: &str, severity: Severity, message: &str, line: usize) -> Failure {
    Failure {
        code: code.to_string(),
        severity,
        message: message.to_string(),
        line,
    }
}

/// Run every rule over the document.
pub fn run_all(doc: &[InstructionPos], config: &Config) -> Vec<Failure> {
    let refs: Vec<&InstructionPos> = doc.iter().collect();
    // The instructions inside ONBUILD, at their lines.
    let unwrapped: Vec<InstructionPos> = doc
        .iter()
        .filter_map(|ip| match &ip.instruction {
            Instruction::OnBuild(inner) => Some(InstructionPos {
                instruction: (**inner).clone(),
                line: ip.line,
            }),
            _ => None,
        })
        .collect();
    let onbuild_refs: Vec<&InstructionPos> = unwrapped.iter().collect();

    let mut out = Vec::new();
    let plain: &[fn(&Doc, &mut Vec<Failure>)] = &[
        dl1001, dl3000, dl3002, dl3003, dl3006, dl3007, dl3010, dl3011, dl3012, dl3020, dl3021,
        dl3022, dl3023, dl3024, dl3025, dl3029, dl3043, dl3044, dl3045, dl3048, dl3057, dl3059,
        dl3061, dl3063, dl3065, dl3066, dl3067, dl4000, dl4003, dl4004, dl4006,
    ];
    let both: &[fn(&Doc, &mut Vec<Failure>)] = &[
        dl3001, dl3004, dl3008, dl3009, dl3013, dl3014, dl3015, dl3016, dl3018, dl3019, dl3027,
        dl3028, dl3030, dl3032, dl3033, dl3034, dl3035, dl3036, dl3037, dl3038, dl3040, dl3041,
        dl3042, dl3046, dl3047, dl3060, dl3062, dl3064, dl4001, dl4005,
    ];
    for rule in plain {
        rule(&refs, &mut out);
    }
    for rule in both {
        rule(&refs, &mut out);
        rule(&onbuild_refs, &mut out);
    }
    dl3026(&refs, &config.trusted_registries, &mut out);
    dl3049(&refs, &config.label_schema, &mut out);
    dl3050(&refs, &config.label_schema, config.strict_labels, &mut out);
    dl3051(&refs, &config.label_schema, &mut out);
    dl3052(&refs, &config.label_schema, &mut out);
    dl3053(&refs, &config.label_schema, &mut out);
    dl3054(&refs, &config.label_schema, &mut out);
    dl3055(&refs, &config.label_schema, &mut out);
    dl3056(&refs, &config.label_schema, &mut out);
    dl3058(&refs, &config.label_schema, &mut out);
    out
}

/// A rule that judges each instruction on its own.
fn simple(
    doc: &Doc,
    out: &mut Vec<Failure>,
    code: &str,
    severity: Severity,
    message: &str,
    ok: impl Fn(&Instruction) -> bool,
) {
    for ip in doc {
        if !ok(&ip.instruction) {
            out.push(failure(code, severity, message, ip.line));
        }
    }
}

const fn run_args(i: &Instruction) -> Option<(&Args, &RunFlags)> {
    match i {
        Instruction::Run { args, flags } => Some((args, flags)),
        _ => None,
    }
}

fn commands(args: &Args) -> &[Command] {
    &args.shell.commands
}

fn command_names(args: &Args) -> Vec<&str> {
    commands(args).iter().map(|c| c.name.as_str()).collect()
}

fn uses_program(args: &Args, prog: &str) -> bool {
    command_names(args).contains(&prog)
}

fn no_commands(args: &Args, pred: impl Fn(&Command) -> bool) -> bool {
    !commands(args).iter().any(pred)
}

fn find_command_index(args: &Args, pred: impl Fn(&Command) -> bool) -> Option<usize> {
    commands(args).iter().position(pred)
}

// ---- DL1001 ---------------------------------------------------------------------

fn dl1001(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL1001",
        Severity::Ignore,
        "Please refrain from using inline ignore pragmas  `# hadolint ignore=DLxxxx`.",
        |i| match i {
            Instruction::Comment(c) => super::parse_ignore_pragma(c, &[]).is_none(),
            _ => true,
        },
    );
}

// ---- DL3000 ---------------------------------------------------------------------

fn is_windows_absolute(path: &str) -> bool {
    let mut chars = path.chars();
    matches!((chars.next(), chars.next()), (Some(d), Some(':')) if d.is_alphabetic())
}

fn dl3000(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3000",
        Severity::Error,
        "Use absolute WORKDIR",
        |i| match i {
            Instruction::Workdir(loc) => {
                let l = drop_quotes(loc);
                l.starts_with('$') || l.starts_with('/') || is_windows_absolute(l)
            }
            _ => true,
        },
    );
}

// ---- DL3001 ---------------------------------------------------------------------

fn dl3001(doc: &Doc, out: &mut Vec<Failure>) {
    const INVALID: &[&str] = &[
        "free", "kill", "mount", "ps", "service", "shutdown", "ssh", "top", "vim",
    ];
    simple(
        doc,
        out,
        "DL3001",
        Severity::Info,
        "For some bash commands it makes no sense running them in a Docker container like `ssh`, `vim`, `shutdown`, `service`, `ps`, `free`, `top`, `kill`, `mount`, `ifconfig`",
        |i| match run_args(i) {
            Some((args, _)) => !command_names(args).iter().any(|n| INVALID.contains(n)),
            None => true,
        },
    );
}

// ---- DL3002 ---------------------------------------------------------------------

fn dl3002(doc: &Doc, out: &mut Vec<Failure>) {
    let is_root =
        |u: &str| u.starts_with("root:") || u.starts_with("0:") || u == "root" || u == "0";
    let mut stage: Option<usize> = None;
    let mut by_stage: BTreeMap<usize, usize> = BTreeMap::new();
    for ip in doc {
        match &ip.instruction {
            Instruction::From(_) => stage = Some(ip.line),
            Instruction::User(u) => {
                if let Some(s) = stage {
                    if is_root(u) {
                        by_stage.insert(s, ip.line);
                    } else {
                        by_stage.remove(&s);
                    }
                }
            }
            _ => {}
        }
    }
    for line in by_stage.values() {
        out.push(failure(
            "DL3002",
            Severity::Warning,
            "Last USER should not be root",
            *line,
        ));
    }
}

// ---- DL3003 / DL3004 --------------------------------------------------------------

fn dl3003(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3003",
        Severity::Warning,
        "Use WORKDIR to switch to a directory",
        |i| run_args(i).is_none_or(|(a, _)| !uses_program(a, "cd")),
    );
}

fn dl3004(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3004",
        Severity::Error,
        "Do not use sudo as it leads to unpredictable behavior. Use a tool like gosu to enforce root",
        |i| run_args(i).is_none_or(|(a, _)| !uses_program(a, "sudo")),
    );
}

// ---- DL3006 / DL3007 --------------------------------------------------------------

fn dl3006(doc: &Doc, out: &mut Vec<Failure>) {
    let mut aliases: BTreeSet<String> = BTreeSet::new();
    for ip in doc {
        if let Instruction::From(from) = &ip.instruction {
            let ok = from.image.name == "scratch"
                || from.digest.is_some()
                || from.tag.is_some()
                || from.image.name.starts_with('$')
                || aliases.contains(&from.image.name);
            if let Some(a) = &from.alias {
                aliases.insert(a.clone());
            }
            if !ok {
                out.push(failure(
                    "DL3006",
                    Severity::Warning,
                    "Always tag the version of an image explicitly",
                    ip.line,
                ));
            }
        }
    }
}

fn dl3007(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3007",
        Severity::Warning,
        "Using latest is prone to errors if the image will ever update. Pin the version explicitly to a release tag",
        |i| match i {
            Instruction::From(BaseImage {
                tag: Some(t),
                digest,
                ..
            }) => digest.is_some() || t != "latest",
            _ => true,
        },
    );
}

// ---- DL3008 ---------------------------------------------------------------------

fn apt_get_packages(args: &Args) -> Vec<String> {
    let mut out = Vec::new();
    for cmd in commands(args) {
        if !cmd.is_with_args("apt-get", &["install"]) {
            continue;
        }
        let dropped = cmd.drop_flag_arg(&["t", "target-release"]);
        for a in dropped.args_no_flags() {
            if a != "install" {
                out.push(a.to_string());
            }
        }
    }
    out
}

fn dl3008(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3008",
        Severity::Warning,
        "Pin versions in apt get install. Instead of `apt-get install <package>` use `apt-get install <package>=<version>`",
        |i| {
            run_args(i).is_none_or(|(a, _)| {
                apt_get_packages(a)
                    .iter()
                    .all(|p| p.contains('=') || p.contains('/') || p.strip_suffix(".deb").is_some())
            })
        },
    );
}

// ---- DL3009 ---------------------------------------------------------------------

fn dl3009(doc: &Doc, out: &mut Vec<Failure>) {
    struct Acc {
        last_from: BaseImage,
        stages: BTreeMap<String, usize>,
        forgets: BTreeMap<usize, BaseImage>,
    }
    let mut acc: Option<Acc> = None;
    let forgot_to_cleanup = |a: &Args| {
        let has_update = commands(a).iter().any(|c| {
            c.is_with_args("apt", &["update"])
                || c.is_with_args("apt-get", &["update"])
                || c.is_with_args("aptitude", &["update"])
        });
        let has_cleanup = commands(a)
            .iter()
            .any(|c| c.is_with_args("rm", &["-rf", "/var/lib/apt/lists/*"]));
        has_update && !has_cleanup
    };
    for ip in doc {
        match &ip.instruction {
            Instruction::From(from) => {
                let name = from.image.name.clone();
                match &mut acc {
                    Some(a) => {
                        a.last_from = from.clone();
                        a.stages.insert(name, ip.line);
                    }
                    None => {
                        acc = Some(Acc {
                            last_from: from.clone(),
                            stages: BTreeMap::from([(name, ip.line)]),
                            forgets: BTreeMap::new(),
                        });
                    }
                }
            }
            Instruction::Run { args, flags } => {
                if !forgot_to_cleanup(args) {
                    continue;
                }
                if flags.has_cache_or_tmpfs_mount_with("/var/lib/apt/lists") {
                    continue;
                }
                if flags.has_cache_or_tmpfs_mount_with("/var/lib/apt")
                    && flags.has_cache_or_tmpfs_mount_with("/var/cache/apt")
                {
                    continue;
                }
                match &mut acc {
                    Some(a) => {
                        a.forgets.insert(ip.line, a.last_from.clone());
                    }
                    None => {
                        acc = Some(Acc {
                            last_from: BaseImage::scratch(),
                            stages: BTreeMap::new(),
                            forgets: BTreeMap::from([(ip.line, BaseImage::scratch())]),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(a) = acc {
        for (line, from) in &a.forgets {
            let used_later = from
                .alias
                .as_ref()
                .is_some_and(|als| a.stages.contains_key(als));
            if *from == a.last_from || used_later {
                out.push(failure(
                    "DL3009",
                    Severity::Info,
                    "Delete the apt lists (/var/lib/apt/lists) after installing something",
                    *line,
                ));
            }
        }
    }
}

// ---- DL3010 ---------------------------------------------------------------------

fn basename(s: &str) -> &str {
    let d = drop_quotes(s);
    d.rsplit(['/', '\\']).next().unwrap_or(d)
}

fn dl3010(doc: &Doc, out: &mut Vec<Failure>) {
    let mut archives: BTreeSet<(usize, String)> = BTreeSet::new();
    let mut extracted: BTreeSet<(usize, String)> = BTreeSet::new();
    let mut started = false;
    for ip in doc {
        match &ip.instruction {
            Instruction::From(_) => {
                archives.clear();
                extracted.clear();
                started = false;
            }
            Instruction::Copy {
                sources,
                target,
                flags,
            } if flags.from.is_none() => {
                started = true;
                if is_archive(target) {
                    archives.insert((ip.line, basename(target).to_string()));
                } else {
                    for s in sources {
                        if is_archive(s) {
                            archives.insert((ip.line, basename(s).to_string()));
                        }
                    }
                }
            }
            Instruction::Run { args, .. } if started => {
                for cmd in commands(args) {
                    let is_tar = cmd.name == "tar"
                        && cmd.get_args().iter().any(|a| {
                            *a == "--extract"
                                || *a == "--get"
                                || (a.starts_with('-') && a.contains('x'))
                        });
                    let is_unzip = matches!(
                        cmd.name.as_str(),
                        "unzip"
                            | "gunzip"
                            | "bunzip2"
                            | "unlzma"
                            | "unxz"
                            | "zgz"
                            | "uncompress"
                            | "zcat"
                            | "gzcat"
                    );
                    if !(is_tar || is_unzip) {
                        continue;
                    }
                    let names: Vec<&str> = cmd.args_no_flags().into_iter().map(basename).collect();
                    for a in &archives {
                        if names.contains(&a.1.as_str()) {
                            extracted.insert(a.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for (line, _) in extracted {
        out.push(failure(
            "DL3010",
            Severity::Info,
            "Use `ADD` for extracting archives into an image",
            line,
        ));
    }
}

// ---- DL3011 / DL3012 ------------------------------------------------------------

fn dl3011(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3011",
        Severity::Error,
        "Valid UNIX ports range from 0 to 65535",
        |i| match i {
            Instruction::Expose(ports) => ports.iter().all(|p| match p {
                PortSpec::Port(Port::Num(n, _)) => *n <= 65535,
                PortSpec::Range(a, b) => [a, b].iter().all(|p| match p {
                    Port::Num(n, _) => *n <= 65535,
                    Port::Var(_) => true,
                }),
                PortSpec::Port(Port::Var(_)) => true,
            }),
            _ => true,
        },
    );
}

fn dl3012(doc: &Doc, out: &mut Vec<Failure>) {
    let mut seen = false;
    for ip in doc {
        match &ip.instruction {
            Instruction::From(_) => seen = false,
            Instruction::Healthcheck(_) => {
                if seen {
                    out.push(failure(
                        "DL3012",
                        Severity::Error,
                        "Multiple `HEALTHCHECK` instructions",
                        ip.line,
                    ));
                } else {
                    seen = true;
                }
            }
            _ => {}
        }
    }
}

// ---- DL3013 ---------------------------------------------------------------------

const PIP_DROP_FLAGS: &[&str] = &[
    "abi",
    "b",
    "build",
    "e",
    "editable",
    "extra-index-url",
    "f",
    "find-links",
    "i",
    "index-url",
    "implementation",
    "no-binary",
    "only-binary",
    "platform",
    "prefix",
    "progress-bar",
    "proxy",
    "python",
    "python-version",
    "root",
    "root-user-action",
    "src",
    "t",
    "target",
    "trusted-host",
    "upgrade-strategy",
];

const VCS_SCHEMES: &[&str] = &[
    "git+file",
    "git+https",
    "git+ssh",
    "git+http",
    "git+git",
    "git",
    "hg+file",
    "hg+http",
    "hg+https",
    "hg+ssh",
    "hg+static-http",
    "svn",
    "svn+svn",
    "svn+http",
    "svn+https",
    "svn+ssh",
    "bzr+http",
    "bzr+https",
    "bzr+ssh",
    "bzr+sftp",
    "bzr+ftp",
    "bzr+lp",
];

fn strip_install_prefix(args: Vec<&str>) -> Vec<&str> {
    let mut it = args.into_iter().skip_while(|a| *a != "install").peekable();
    while it.peek() == Some(&"install") {
        it.next();
    }
    it.collect()
}

fn dl3013(doc: &Doc, out: &mut Vec<Failure>) {
    let version_fixed = |p: &str| {
        let is_vcs = VCS_SCHEMES.iter().any(|s| p.starts_with(s));
        ["==", ">=", "<=", ">", "<", "!=", "~=", "==="]
            .iter()
            .any(|s| p.contains(s))
            || (is_vcs && p.contains('@'))
            || p.strip_suffix(".whl").is_some()
            || p.strip_suffix(".tar.gz").is_some()
            || (!is_vcs && p.contains('/'))
    };
    let forgot = |cmd: &Command| {
        let has_constraint = cmd.has_flag("constraint") || cmd.has_flag("c");
        let packages = strip_install_prefix(cmd.drop_flag_arg(PIP_DROP_FLAGS).args_no_flags())
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let all_fixed = packages.iter().all(|p| version_fixed(p));
        let args = cmd.get_args();
        let requirement = contains_seq(&args, &["--requirement"])
            || contains_seq(&args, &["-r"])
            || contains_seq(&args, &["."]);
        let is_pip_install = cmd.is_pip_install() && !has_constraint && !all_fixed && !requirement;
        is_pip_install && !has_constraint && !all_fixed
    };
    simple(
        doc,
        out,
        "DL3013",
        Severity::Warning,
        "Pin versions in pip. Instead of `pip install <package>` use `pip install <package>==<version>` or `pip install --requirement <requirements file>`",
        |i| run_args(i).is_none_or(|(a, _)| no_commands(a, |c| forgot(c))),
    );
}

// ---- DL3014 / DL3015 ------------------------------------------------------------

fn dl3014(doc: &Doc, out: &mut Vec<Failure>) {
    let forgot = |cmd: &Command| {
        cmd.is_with_args("apt-get", &["install"])
            && !(cmd.has_any_flag(&["y", "yes", "qq", "assume-yes"])
                || cmd.count_flag("q") == 2
                || cmd.count_flag("quiet") == 2
                || cmd.get_args().contains(&"-q=2"))
    };
    simple(
        doc,
        out,
        "DL3014",
        Severity::Warning,
        "Use the `-y` switch to avoid manual input `apt-get -y install <package>`",
        |i| run_args(i).is_none_or(|(a, _)| no_commands(a, |c| forgot(c))),
    );
}

fn dl3015(doc: &Doc, out: &mut Vec<Failure>) {
    let forgot = |cmd: &Command| {
        cmd.is_with_args("apt-get", &["install"])
            && !(cmd.has_flag("no-install-recommends")
                || cmd.has_arg("APT::Install-Recommends=false"))
    };
    simple(
        doc,
        out,
        "DL3015",
        Severity::Info,
        "Avoid additional packages by specifying `--no-install-recommends`",
        |i| run_args(i).is_none_or(|(a, _)| no_commands(a, |c| forgot(c))),
    );
}

// ---- DL3016 ---------------------------------------------------------------------

fn dl3016(doc: &Doc, out: &mut Vec<Failure>) {
    const IGNORE_FLAGS: &[&str] = &["loglevel", "registry"];
    let version_fixed = |p: &str| {
        if ["git://", "git+ssh://", "git+http://", "git+https://"]
            .iter()
            .any(|g| p.starts_with(g))
        {
            return p.contains('#');
        }
        if [".tar", ".tar.gz", ".tgz"].iter().any(|s| p.ends_with(s)) {
            return true;
        }
        if ["/", "./", "../", "~/"].iter().any(|s| p.starts_with(s)) {
            return true;
        }
        let scoped = if p.starts_with('@') {
            p.trim_start_matches(|c: char| c > '/')
        } else {
            p
        };
        scoped.contains('@')
    };
    let forgot = |cmd: &Command| {
        if !cmd.is_with_args("npm", &["install"]) {
            return false;
        }
        let dropped = cmd.drop_flag_arg(IGNORE_FLAGS);
        let no_flags = dropped.args_no_flags();
        let install_first = no_flags.first() == Some(&"install");
        let packages = strip_install_prefix(no_flags);
        install_first && !packages.iter().all(|p| version_fixed(p))
    };
    simple(
        doc,
        out,
        "DL3016",
        Severity::Warning,
        "Pin versions in npm. Instead of `npm install <package>` use `npm install <package>@<version>`",
        |i| run_args(i).is_none_or(|(a, _)| no_commands(a, |c| forgot(c))),
    );
}

// ---- DL3018 / DL3019 ------------------------------------------------------------

fn apk_add_packages(args: &Args) -> Vec<String> {
    let mut out = Vec::new();
    for cmd in commands(args) {
        if !cmd.is_with_args("apk", &["add"]) {
            continue;
        }
        let dropped = cmd.drop_flag_arg(&["t", "virtual", "repository", "X"]);
        for a in dropped.args_no_flags() {
            if a != "add" {
                out.push(a.to_string());
            }
        }
    }
    out
}

fn dl3018(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3018",
        Severity::Warning,
        "Pin versions in apk add. Instead of `apk add <package>` use `apk add <package>=<version>`",
        |i| {
            run_args(i).is_none_or(|(a, _)| {
                apk_add_packages(a).iter().all(|p| {
                    ["=", "~", ">", "<"].iter().any(|s| p.contains(s))
                        || p.strip_suffix(".apk").is_some()
                })
            })
        },
    );
}

fn dl3019(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3019",
        Severity::Info,
        "Use the `--no-cache` switch to avoid the need to use `--update` and remove `/var/cache/apk/*` when done installing packages",
        |i| {
            run_args(i).is_none_or(|(a, flags)| {
                flags.has_cache_or_tmpfs_mount_with("/var/cache/apk")
                    || no_commands(a, |c| {
                        c.is_with_args("apk", &["add"]) && !c.has_flag("no-cache")
                    })
            })
        },
    );
}

// ---- DL3020 - DL3025 ------------------------------------------------------------

fn dl3020(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3020",
        Severity::Error,
        "Use COPY instead of ADD for files and folders",
        |i| match i {
            Instruction::Add { sources, .. } => sources.iter().all(|s| {
                let d = drop_quotes(s);
                is_archive(s) || d.starts_with("https://") || d.starts_with("http://")
            }),
            _ => true,
        },
    );
}

fn dl3021(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3021",
        Severity::Error,
        "COPY with more than 2 arguments requires the last argument to end with /",
        |i| match i {
            Instruction::Copy {
                sources, target, ..
            } if sources.len() > 1 => !target.is_empty() && drop_quotes(target).ends_with('/'),
            _ => true,
        },
    );
}

fn dl3022(doc: &Doc, out: &mut Vec<Failure>) {
    let mut count = 0usize;
    let mut names: BTreeSet<String> = BTreeSet::new();
    for ip in doc {
        match &ip.instruction {
            Instruction::From(from) => {
                count += 1;
                if let Some(a) = &from.alias {
                    names.insert(a.clone());
                }
            }
            Instruction::Copy { flags, .. } => {
                let Some(src) = &flags.from else { continue };
                if drop_quotes(src).contains(':') || names.contains(src) {
                    continue;
                }
                let digits: String = src.chars().take_while(char::is_ascii_digit).collect();
                if !digits.is_empty()
                    && let Ok(v) = digits.parse::<usize>()
                    && v < count
                {
                    continue;
                }
                out.push(failure(
                    "DL3022",
                    Severity::Warning,
                    "`COPY --from` should reference a previously defined `FROM` alias",
                    ip.line,
                ));
            }
            _ => {}
        }
    }
}

fn dl3023(doc: &Doc, out: &mut Vec<Failure>) {
    let mut last_alias: Option<Option<String>> = None;
    for ip in doc {
        match &ip.instruction {
            Instruction::From(from) => last_alias = Some(from.alias.clone()),
            Instruction::Copy { flags, .. } => {
                if let (Some(Some(alias)), Some(src)) = (&last_alias, &flags.from)
                    && alias == src
                {
                    out.push(failure(
                        "DL3023",
                        Severity::Error,
                        "`COPY --from` cannot reference its own `FROM` alias",
                        ip.line,
                    ));
                }
            }
            _ => {}
        }
    }
}

fn dl3024(doc: &Doc, out: &mut Vec<Failure>) {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for ip in doc {
        if let Instruction::From(BaseImage { alias: Some(a), .. }) = &ip.instruction {
            if seen.contains(a) {
                out.push(failure(
                    "DL3024",
                    Severity::Error,
                    "FROM aliases (stage names) must be unique",
                    ip.line,
                ));
            } else {
                seen.insert(a.clone());
            }
        }
    }
}

fn dl3025(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3025",
        Severity::Warning,
        "Use arguments JSON notation for CMD and ENTRYPOINT arguments",
        |i| match i {
            Instruction::Cmd(a) | Instruction::Entrypoint(a) => a.kind != ArgsKind::Text,
            Instruction::Healthcheck(Some(c)) => c.command.kind != ArgsKind::Text,
            _ => true,
        },
    );
}

// ---- DL3026 ---------------------------------------------------------------------

fn match_registry(allow: &str, registry: &str) -> bool {
    if allow == "*" {
        return true;
    }
    if let Some(suffix) = allow.strip_prefix('*') {
        return registry.ends_with(suffix);
    }
    if let Some(prefix) = allow.strip_suffix('*') {
        return registry.starts_with(prefix);
    }
    registry == allow
}

fn dl3026(doc: &Doc, allowed: &[String], out: &mut Vec<Failure>) {
    let registry_allowed = |r: &str| allowed.iter().any(|a| match_registry(a, r));
    let is_allowed = |img: &Image| match &img.registry {
        Some(r) => registry_allowed(r),
        None => {
            img.name == "scratch"
                || registry_allowed("docker.io")
                || registry_allowed("hub.docker.com")
        }
    };
    let mut aliases: BTreeSet<Option<String>> = BTreeSet::new();
    let check = |aliases: &BTreeSet<Option<String>>, img: &Image| {
        aliases.contains(&Some(img.name.clone())) || allowed.is_empty() || is_allowed(img)
    };
    let msg = "Use only an allowed registry in the FROM image";
    for ip in doc {
        match &ip.instruction {
            Instruction::From(from) => {
                let ok = check(&aliases, &from.image);
                aliases.insert(from.alias.clone());
                if !ok {
                    out.push(failure("DL3026", Severity::Error, msg, ip.line));
                }
            }
            Instruction::Copy { flags, .. } => {
                if let Some(src) = &flags.from
                    && !check(&aliases, &Image::from_text(src))
                {
                    out.push(failure("DL3026", Severity::Error, msg, ip.line));
                }
            }
            Instruction::Run { flags, .. } => {
                let bad = flags.mounts.iter().any(|m| match m {
                    RunMount::Cache { from, .. } | RunMount::Bind { from, .. } => {
                        let img = Image::from_text(from.as_deref().unwrap_or("scratch"));
                        !check(&aliases, &img)
                    }
                    _ => false,
                });
                if bad {
                    out.push(failure("DL3026", Severity::Error, msg, ip.line));
                }
            }
            _ => {}
        }
    }
}

// ---- DL3027 / DL3028 ------------------------------------------------------------

fn dl3027(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3027",
        Severity::Warning,
        "Do not use apt as it is meant to be an end-user tool, use apt-get or apt-cache instead",
        |i| run_args(i).is_none_or(|(a, _)| !uses_program(a, "apt")),
    );
}

fn gems(args: &Args) -> Vec<String> {
    let mut out = Vec::new();
    for cmd in commands(args) {
        if !cmd.is_with_args("gem", &["install", "i"])
            || cmd.is_with_args("gem", &["-v"])
            || cmd.is_with_args("gem", &["--version"])
            || cmd.has_prefix_arg("gem", "--version=")
        {
            continue;
        }
        let all = cmd.get_args();
        let until: Vec<&str> = all.iter().take_while(|a| **a != "--").copied().collect();
        let mut rest = until.as_slice();
        while let Some((x, xs)) = rest.split_first() {
            if *x == "--" {
                rest = xs;
            } else if x.starts_with('-') {
                rest = xs.get(1..).unwrap_or(&[]);
            } else {
                if *x != "install" && *x != "i" {
                    out.push((*x).to_string());
                }
                rest = xs;
            }
        }
    }
    out
}

fn dl3028(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3028",
        Severity::Warning,
        "Pin versions in gem install. Instead of `gem install <gem>` use `gem install <gem>:<version>`",
        |i| run_args(i).is_none_or(|(a, _)| gems(a).iter().all(|g| g.contains(':'))),
    );
}

// ---- DL3029 / DL3030 ------------------------------------------------------------

fn dl3029(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3029",
        Severity::Warning,
        "Do not use --platform flag with FROM",
        |i| match i {
            Instruction::From(BaseImage {
                platform: Some(p), ..
            }) => p.contains("BUILDPLATFORM") || p.contains("TARGETPLATFORM"),
            _ => true,
        },
    );
}

fn dl3030(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3030",
        Severity::Warning,
        "Use the -y switch to avoid manual input `yum install -y <package>`",
        |i| {
            run_args(i).is_none_or(|(a, _)| {
                no_commands(a, |c| {
                    c.is_with_args("yum", &["install", "groupinstall", "localinstall"])
                        && !c.has_any_flag(&["y", "assumeyes"])
                })
            })
        },
    );
}

// ---- DL3032 ---------------------------------------------------------------------

fn dl3032(doc: &Doc, out: &mut Vec<Failure>) {
    let install = |c: &Command| c.is_with_args("yum", &["install"]);
    let clean = |c: &Command| {
        c.is_with_args("yum", &["clean", "all"])
            || c.is_with_args("rm", &["-rf", "/var/cache/yum/*"])
    };
    simple(
        doc,
        out,
        "DL3032",
        Severity::Warning,
        "`yum clean all` missing after yum command.",
        |i| {
            run_args(i).is_none_or(|(a, _)| {
                no_commands(a, install)
                    || matches!(
                        (find_command_index(a, install), find_command_index(a, clean)),
                        (Some(x), Some(y)) if x < y
                    )
            })
        },
    );
}

// ---- DL3033 / DL3041 (shared machinery) ------------------------------------------

struct VersionAcc {
    stage_idx: usize,
    stage_aliases: BTreeMap<usize, String>,
    envs: BTreeMap<usize, BTreeSet<String>>,
    args: BTreeSet<String>,
}

impl VersionAcc {
    fn register_envs(acc: &mut Option<Self>, pairs: &Pairs) {
        let keys: BTreeSet<String> = pairs.iter().map(|(k, _)| k.clone()).collect();
        match acc {
            None => {
                *acc = Some(Self {
                    stage_idx: 0,
                    stage_aliases: BTreeMap::from([(0, String::new())]),
                    envs: BTreeMap::from([(0, keys)]),
                    args: BTreeSet::new(),
                });
            }
            Some(a) => {
                if let Some(set) = a.envs.get_mut(&a.stage_idx) {
                    set.extend(keys);
                }
            }
        }
    }

    fn register_arg(acc: &mut Option<Self>, name: &str) {
        match acc {
            None => {
                *acc = Some(Self {
                    stage_idx: 0,
                    stage_aliases: BTreeMap::from([(0, String::new())]),
                    envs: BTreeMap::from([(0, BTreeSet::new())]),
                    args: BTreeSet::from([name.to_string()]),
                });
            }
            Some(a) => {
                a.args.insert(name.to_string());
            }
        }
    }

    fn new_stage(acc: &mut Option<Self>, image: &BaseImage) {
        let alias = image.alias.clone().unwrap_or_default();
        match acc {
            None => {
                *acc = Some(Self {
                    stage_idx: 1,
                    stage_aliases: BTreeMap::from([(1, alias)]),
                    envs: BTreeMap::from([(1, BTreeSet::new())]),
                    args: BTreeSet::new(),
                });
            }
            Some(a) => {
                let next = a.stage_idx + 1;
                let parent_idx = a
                    .stage_aliases
                    .iter()
                    .find(|(_, n)| **n == image.image.name)
                    .map_or(next, |(i, _)| *i);
                let inherited = a.envs.get(&parent_idx).cloned().unwrap_or_default();
                a.stage_idx = next;
                a.stage_aliases.insert(next, alias);
                a.envs.insert(next, inherited);
            }
        }
    }

    fn env_defined(acc: Option<&Self>, package: &str) -> bool {
        let Some(a) = acc else { return false };
        let var_in = |v: &str| package.contains(&format!("${{{v}}}"));
        a.envs
            .get(&a.stage_idx)
            .is_some_and(|s| s.iter().any(|v| var_in(v)))
            || a.args.iter().any(|v| var_in(v))
    }

    fn package_version_fixed(acc: Option<&Self>, package: &str) -> bool {
        let parts: Vec<&str> = package.split('-').collect();
        if parts.len() <= 1 {
            return false;
        }
        if package.strip_suffix(".rpm").is_some() {
            return true;
        }
        if package.contains('$') {
            return Self::env_defined(acc, package);
        }
        let rest = &parts[1..];
        let is_version_char =
            |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '~' | '^' | '_' | ':' | '+');
        rest.iter().all(|p| p.chars().all(is_version_char))
            && rest
                .iter()
                .any(|p| p.chars().next().is_some_and(|c| c.is_ascii_digit()))
    }
}

fn version_rule(
    doc: &Doc,
    out: &mut Vec<Failure>,
    code: &str,
    message: &str,
    cmds: &[&str],
    is_module_cmd: &dyn Fn(&Command) -> bool,
) {
    let mut acc: Option<VersionAcc> = None;
    for ip in doc {
        match &ip.instruction {
            Instruction::Run { args, .. } => {
                let install_filter = |cmd: &Command| -> Vec<String> {
                    if !cmd.is_any_with_args(cmds, &["install"]) {
                        return Vec::new();
                    }
                    cmd.args_no_flags()
                        .into_iter()
                        .filter(|a| *a != "install" && *a != "module")
                        .map(str::to_string)
                        .collect()
                };
                let packages: Vec<String> = commands(args)
                    .iter()
                    .filter(|c| !is_module_cmd(c))
                    .flat_map(install_filter)
                    .collect();
                let modules: Vec<String> = commands(args)
                    .iter()
                    .filter(|c| c.is_any_with_args(cmds, &["module"]))
                    .flat_map(install_filter)
                    .collect();
                let ok = packages
                    .iter()
                    .all(|p| VersionAcc::package_version_fixed(acc.as_ref(), p))
                    && modules.iter().all(|m| m.contains(':'));
                if !ok {
                    out.push(failure(code, Severity::Warning, message, ip.line));
                }
            }
            Instruction::Env(pairs) => VersionAcc::register_envs(&mut acc, pairs),
            Instruction::Arg { name, .. } => VersionAcc::register_arg(&mut acc, name),
            Instruction::From(bi) => VersionAcc::new_stage(&mut acc, bi),
            _ => {}
        }
    }
}

fn dl3033(doc: &Doc, out: &mut Vec<Failure>) {
    version_rule(
        doc,
        out,
        "DL3033",
        "Specify version with `yum install -y <package>-<version>`.",
        &["yum"],
        &|c| c.is_with_args("yum", &["module"]),
    );
}

fn dl3041(doc: &Doc, out: &mut Vec<Failure>) {
    version_rule(
        doc,
        out,
        "DL3041",
        "Specify version with `dnf install -y <package>-<version>`.",
        &["dnf", "microdnf"],
        &|c| c.is_any_with_args(&["dnf", "microdnf"], &["module", "group"]),
    );
}

// ---- DL3034 - DL3038 ------------------------------------------------------------

fn dl3034(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3034",
        Severity::Warning,
        "Non-interactive switch missing from `zypper` command: `zypper install -y`",
        |i| {
            run_args(i).is_none_or(|(a, _)| {
                no_commands(a, |c| {
                    c.is_with_args(
                        "zypper",
                        &[
                            "install",
                            "in",
                            "remove",
                            "rm",
                            "source-install",
                            "si",
                            "patch",
                        ],
                    ) && !c.has_any_flag(&["non-interactive", "n", "no-confirm", "y"])
                })
            })
        },
    );
}

fn dl3035(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3035",
        Severity::Warning,
        "Do not use `zypper dist-upgrade`.",
        |i| {
            run_args(i).is_none_or(|(a, _)| {
                no_commands(a, |c| c.is_with_args("zypper", &["dist-upgrade", "dup"]))
            })
        },
    );
}

fn dl3036(doc: &Doc, out: &mut Vec<Failure>) {
    let install = |c: &Command| c.is_with_args("zypper", &["install", "in"]);
    let clean = |c: &Command| c.is_with_args("zypper", &["clean", "cc"]);
    simple(
        doc,
        out,
        "DL3036",
        Severity::Warning,
        "`zypper clean` missing after zypper use.",
        |i| {
            run_args(i).is_none_or(|(a, flags)| {
                no_commands(a, install)
                    || flags.has_cache_or_tmpfs_mount_with("/var/cache/zypp")
                    || matches!(
                        (find_command_index(a, install), find_command_index(a, clean)),
                        (Some(x), Some(y)) if x < y
                    )
            })
        },
    );
}

fn dl3037(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3037",
        Severity::Warning,
        "Specify version with `zypper install -y <package>=<version>`.",
        |i| {
            run_args(i).is_none_or(|(a, _)| {
                commands(a)
                    .iter()
                    .filter(|c| c.is_with_args("zypper", &["install", "in"]))
                    .flat_map(|c| {
                        c.args_no_flags()
                            .into_iter()
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                    .filter(|p| p != "install" && p != "in")
                    .all(|p| {
                        ["=", ">=", ">", "<=", "<"].iter().any(|s| p.contains(s))
                            || p.strip_suffix(".rpm").is_some()
                    })
            })
        },
    );
}

fn dl3038(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3038",
        Severity::Warning,
        "Use the -y switch to avoid manual input `dnf install -y <package>`",
        |i| {
            run_args(i).is_none_or(|(a, _)| {
                no_commands(a, |c| {
                    c.is_any_with_args(
                        &["dnf", "microdnf"],
                        &["install", "groupinstall", "localinstall"],
                    ) && !c.has_any_flag(&["y", "assumeyes"])
                })
            })
        },
    );
}

// ---- DL3040 ---------------------------------------------------------------------

fn dl3040(doc: &Doc, out: &mut Vec<Failure>) {
    const INSTALL: &[&str] = &[
        "install",
        "in",
        "upgrade",
        "up",
        "upgrade-minimal",
        "up-min",
        "reinstall",
        "rei",
    ];
    let missing_clean_ok = |a: &Args, name: &str| {
        let install = |c: &Command| c.is_with_args(name, INSTALL);
        let clean = |c: &Command| {
            c.is_with_args(name, &["clean", "all"])
                || c.is_with_args("rm", &["-rf", "/var/cache/libdnf5*"])
        };
        no_commands(a, install)
            || matches!(
                (find_command_index(a, install), find_command_index(a, clean)),
                (Some(x), Some(y)) if x < y
            )
    };
    simple(
        doc,
        out,
        "DL3040",
        Severity::Warning,
        "`dnf clean all` missing after dnf command.",
        |i| {
            run_args(i).is_none_or(|(a, flags)| {
                ["dnf", "microdnf"].iter().all(|n| missing_clean_ok(a, n))
                    || flags.has_cache_or_tmpfs_mount_with("/var/cache/libdnf5")
                    || flags.has_cache_or_tmpfs_mount_with(".cache/libdnf5")
            })
        },
    );
}

// ---- DL3042 ---------------------------------------------------------------------

const TRUTHY: &[&str] = &[
    "1", "true", "True", "TRUE", "on", "On", "ON", "yes", "Yes", "YES",
];

fn dl3042(doc: &Doc, out: &mut Vec<Failure>) {
    // Stage name: Some(name) or None (the ONBUILD context without a FROM)
    let mut current: Option<Option<String>> = None;
    let mut no_cache: BTreeMap<Option<String>, bool> = BTreeMap::new();
    let mut started = false;
    let pip_no_cache_dir_set = |pairs: &Pairs| {
        pairs
            .iter()
            .find(|(k, _)| k == "PIP_NO_CACHE_DIR")
            .is_some_and(|(_, v)| TRUTHY.contains(&v.as_str()))
    };
    let pip_no_cache_in_text = |shell: &ParsedShell| {
        let Some(idx) = shell.original.find("PIP_NO_CACHE_DIR=") else {
            return false;
        };
        let after = &shell.original[idx..];
        let after = after.find('=').map_or("", |p| &after[p + 1..]);
        TRUTHY.iter().any(|t| after.starts_with(t))
    };
    let forgot = |cmd: &Command| {
        let is_wrapper = |w: &str| {
            cmd.name.contains(w)
                || (cmd.name.starts_with("python") && contains_seq(&cmd.get_args(), &["-m", w]))
        };
        cmd.is_pip_install()
            && !cmd.get_args().contains(&"--no-cache-dir")
            && !(is_wrapper("pipx") || is_wrapper("pipenv"))
    };
    for ip in doc {
        match &ip.instruction {
            Instruction::From(from) => {
                let name = from
                    .alias
                    .clone()
                    .unwrap_or_else(|| from.image.name.clone());
                if started {
                    let parent = no_cache
                        .get(&Some(from.image.name.clone()))
                        .copied()
                        .unwrap_or(false);
                    no_cache.insert(Some(name.clone()), parent);
                }
                current = Some(Some(name));
                started = true;
            }
            Instruction::Env(pairs) => {
                if pip_no_cache_dir_set(pairs) {
                    if !started {
                        current = Some(None);
                        started = true;
                    }
                    if let Some(c) = &current {
                        no_cache.insert(c.clone(), true);
                    }
                }
            }
            Instruction::Run { args, flags } => {
                let set = started
                    && current
                        .as_ref()
                        .is_some_and(|c| no_cache.get(c) == Some(&true));
                if set
                    || pip_no_cache_in_text(&args.shell)
                    || no_commands(args, |c| forgot(c))
                    || flags.has_cache_or_tmpfs_mount_with(".cache/pip")
                {
                    continue;
                }
                out.push(failure(
                    "DL3042",
                    Severity::Warning,
                    "Avoid use of cache directory with pip. Use `pip install --no-cache-dir <package>`",
                    ip.line,
                ));
            }
            _ => {}
        }
    }
}

// ---- DL3043 - DL3048 ------------------------------------------------------------

fn dl3043(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3043",
        Severity::Error,
        "`ONBUILD`, `FROM` or `MAINTAINER` triggered from within `ONBUILD` instruction.",
        |i| match i {
            Instruction::OnBuild(inner) => !matches!(
                **inner,
                Instruction::OnBuild(_) | Instruction::From(_) | Instruction::Maintainer(_)
            ),
            _ => true,
        },
    );
}

const fn is_var_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn bare_variable_in_text(v: &str, t: &str) -> bool {
    let var = format!("${v}");
    if !t.contains(&var) {
        return false;
    }
    t.split(&var)
        .skip(1)
        .any(|rest| rest.chars().next().is_none_or(|c| !is_var_char(c)))
}

fn dl3044(doc: &Doc, out: &mut Vec<Failure>) {
    let mut defined: BTreeSet<String> = BTreeSet::new();
    for ip in doc {
        match &ip.instruction {
            Instruction::Env(pairs) => {
                let referenced: Vec<&String> = pairs
                    .iter()
                    .enumerate()
                    .filter(|(idx, (var, _))| {
                        pairs.iter().enumerate().any(|(j, (_, value))| {
                            j != *idx
                                && (value.contains(&format!("${{{var}}}"))
                                    || bare_variable_in_text(var, value))
                        })
                    })
                    .map(|(_, (var, _))| var)
                    .collect();
                let bad = referenced.iter().any(|v| !defined.contains(*v));
                defined.extend(pairs.iter().map(|(k, _)| k.clone()));
                if bad {
                    out.push(failure(
                        "DL3044",
                        Severity::Error,
                        "Do not refer to an environment variable within the same `ENV` statement where it is defined.",
                        ip.line,
                    ));
                }
            }
            Instruction::Arg { name, .. } => {
                defined.insert(name.clone());
            }
            _ => {}
        }
    }
}

fn dl3045(doc: &Doc, out: &mut Vec<Failure>) {
    let mut current: Option<Option<String>> = None;
    let mut workdir_set: BTreeMap<Option<String>, bool> = BTreeMap::new();
    let mut started = false;
    for ip in doc {
        match &ip.instruction {
            Instruction::From(from) => {
                let name = from
                    .alias
                    .clone()
                    .unwrap_or_else(|| from.image.name.clone());
                if started {
                    let parent = workdir_set
                        .get(&Some(from.image.name.clone()))
                        .copied()
                        .unwrap_or(false);
                    workdir_set.insert(Some(name.clone()), parent);
                }
                current = Some(Some(name));
                started = true;
            }
            Instruction::Workdir(_) => {
                if !started {
                    current = Some(None);
                    started = true;
                }
                if let Some(c) = &current {
                    workdir_set.insert(c.clone(), true);
                }
            }
            Instruction::Copy { target, .. } => {
                let set = current
                    .as_ref()
                    .is_some_and(|c| workdir_set.get(c) == Some(&true));
                let dest = drop_quotes(target);
                if set
                    || dest.starts_with('/')
                    || is_windows_absolute(dest)
                    || dest.starts_with('$')
                {
                    continue;
                }
                out.push(failure(
                    "DL3045",
                    Severity::Warning,
                    "`COPY` to a relative destination without `WORKDIR` set.",
                    ip.line,
                ));
            }
            _ => {}
        }
    }
}

fn dl3046(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3046",
        Severity::Warning,
        "`useradd` without flag `-l` and high UID will result in excessively large Image.",
        |i| {
            run_args(i).is_none_or(|(a, _)| {
                no_commands(a, |c| {
                    c.name == "useradd"
                        && !c.has_any_flag(&["l", "no-log-init"])
                        && c.has_any_flag(&["u", "uid"])
                        && c.flag_args("u").iter().any(|v| v.chars().count() > 5)
                })
            })
        },
    );
}

fn dl3047(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3047",
        Severity::Info,
        "Avoid use of wget without progress bar. Use `wget --progress=dot:giga <url>`. Or consider using `-q` or `-nv` (shorthands for `--quiet` or `--no-verbose`).",
        |i| {
            run_args(i).is_none_or(|(a, _)| {
                no_commands(a, |c| {
                    c.name == "wget"
                        && !c.has_flag("progress")
                        && !(c.has_any_flag(&["q", "quiet"])
                            || c.has_any_flag(&["o", "output-file"])
                            || c.has_any_flag(&["a", "append-output"])
                            || c.has_any_flag(&["no-verbose"])
                            || c.is_with_args("wget", &["-nv"]))
                })
            })
        },
    );
}

fn dl3048(doc: &Doc, out: &mut Vec<Failure>) {
    let valid_char = |c: char| {
        c.is_ascii_digit() || c.is_ascii_lowercase() || matches!(c, '.' | '-' | '_' | '/')
    };
    simple(
        doc,
        out,
        "DL3048",
        Severity::Style,
        "Invalid label key.",
        |i| match i {
            Instruction::Label(pairs) => !pairs.iter().any(|(l, _)| {
                let first_ok = l.chars().next().is_some_and(|c| c.is_ascii_lowercase());
                let last_ok = l
                    .chars()
                    .last()
                    .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
                !first_ok
                    || !last_ok
                    || l.chars().any(|c| !valid_char(c))
                    || l.starts_with("com.docker.")
                    || l.starts_with("io.docker.")
                    || l.starts_with("org.dockerproject.")
                    || l.contains("..")
                    || l.contains("--")
            }),
            _ => true,
        },
    );
}

// ---- DL3049 - DL3058: label schema ------------------------------------------------

fn dl3049(doc: &Doc, schema: &[(String, LabelType)], out: &mut Vec<Failure>) {
    #[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct StageId {
        image: BaseImage,
        line: usize,
    }
    for (label, _) in schema {
        let mut current: Option<StageId> = None;
        let mut good: BTreeSet<StageId> = BTreeSet::new();
        let mut silent: BTreeSet<StageId> = BTreeSet::new();
        let mut bad: BTreeSet<StageId> = BTreeSet::new();
        let stage_name = |s: &StageId| {
            s.image
                .alias
                .clone()
                .unwrap_or_else(|| s.image.image.name.clone())
        };
        let mark_silent =
            |name: &str, bad: &mut BTreeSet<StageId>, silent: &mut BTreeSet<StageId>| {
                let stages: Vec<StageId> = bad
                    .iter()
                    .filter(|s| s.image.alias.as_deref() == Some(name))
                    .cloned()
                    .collect();
                for s in stages {
                    bad.remove(&s);
                    silent.insert(s);
                }
            };
        for ip in doc {
            match &ip.instruction {
                Instruction::From(img) => {
                    let id = StageId {
                        image: img.clone(),
                        line: ip.line,
                    };
                    let inherits_good = current.is_some()
                        && good
                            .iter()
                            .any(|g| g.image.alias.as_deref() == Some(img.image.name.as_str()));
                    if inherits_good {
                        good.insert(id.clone());
                    } else {
                        bad.insert(id.clone());
                    }
                    current = Some(id);
                }
                Instruction::Copy { flags, .. } => {
                    if let Some(src) = &flags.from
                        && current.is_some()
                    {
                        mark_silent(src, &mut bad, &mut silent);
                    }
                }
                Instruction::Label(pairs) => {
                    if pairs.iter().any(|(k, _)| k == label)
                        && let Some(cur) = &current
                    {
                        let name = stage_name(cur);
                        mark_silent(&name, &mut bad, &mut silent);
                        good.insert(cur.clone());
                        silent.remove(cur);
                        bad.remove(cur);
                    }
                }
                _ => {}
            }
        }
        for s in bad {
            out.push(failure(
                "DL3049",
                Severity::Info,
                &format!("Label `{label}` is missing."),
                s.line,
            ));
        }
    }
}

fn dl3050(doc: &Doc, schema: &[(String, LabelType)], strict: bool, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3050",
        Severity::Info,
        "Superfluous label(s) present.",
        |i| match i {
            Instruction::Label(pairs) if strict => pairs
                .iter()
                .all(|(k, _)| schema.iter().any(|(n, _)| n == k)),
            _ => true,
        },
    );
}

fn label_rule(
    doc: &Doc,
    schema: &[(String, LabelType)],
    out: &mut Vec<Failure>,
    code: &str,
    ty: Option<LabelType>,
    message: &dyn Fn(&str) -> String,
    bad: &dyn Fn(&str) -> bool,
) {
    for (label, lt) in schema {
        if ty.is_some_and(|t| t != *lt) {
            continue;
        }
        simple(
            doc,
            out,
            code,
            Severity::Warning,
            &message(label),
            |i| match i {
                Instruction::Label(pairs) => !pairs.iter().any(|(l, v)| l == label && bad(v)),
                _ => true,
            },
        );
    }
}

fn dl3051(doc: &Doc, schema: &[(String, LabelType)], out: &mut Vec<Failure>) {
    label_rule(
        doc,
        schema,
        out,
        "DL3051",
        None,
        &|l| format!("label `{l}` is empty."),
        &str::is_empty,
    );
}

/// RFC 3986 `parseURI`: an absolute URI with a scheme and valid characters.
fn is_uri(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once(':') else {
        return false;
    };
    let mut sc = scheme.chars();
    if !sc.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if !sc.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return false;
    }
    let allowed = |c: char| c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c);
    if !rest.chars().all(allowed) {
        return false;
    }
    // Percent escapes must be complete.
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() && i + 2 > bytes.len() - 1 {
                return false;
            }
            if !(bytes[i + 1].is_ascii_hexdigit() && bytes[i + 2].is_ascii_hexdigit()) {
                return false;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    // At most one '?' before the fragment and one '#'.
    rest.matches('#').count() <= 1
}

fn dl3052(doc: &Doc, schema: &[(String, LabelType)], out: &mut Vec<Failure>) {
    label_rule(
        doc,
        schema,
        out,
        "DL3052",
        Some(LabelType::Url),
        &|l| format!("Label `{l}` is not a valid URL."),
        &|v| !is_uri(v),
    );
}

fn is_rfc3339(s: &str) -> bool {
    let b = s.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| r.end <= b.len() && b[r].iter().all(u8::is_ascii_digit);
    if !(digits(0..4)
        && b.get(4) == Some(&b'-')
        && digits(5..7)
        && b.get(7) == Some(&b'-')
        && digits(8..10))
    {
        return false;
    }
    if !matches!(b.get(10), Some(b'T' | b't' | b' ')) {
        return false;
    }
    if !(digits(11..13)
        && b.get(13) == Some(&b':')
        && digits(14..16)
        && b.get(16) == Some(&b':')
        && digits(17..19))
    {
        return false;
    }
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    match b.get(i) {
        Some(b'Z' | b'z') => i + 1 == b.len(),
        Some(b'+' | b'-') => {
            digits(i + 1..i + 3)
                && b.get(i + 3) == Some(&b':')
                && digits(i + 4..i + 6)
                && i + 6 == b.len()
        }
        _ => false,
    }
}

fn dl3053(doc: &Doc, schema: &[(String, LabelType)], out: &mut Vec<Failure>) {
    label_rule(
        doc,
        schema,
        out,
        "DL3053",
        Some(LabelType::Rfc3339),
        &|l| format!("Label `{l}` is not a valid time format - must conform to RFC3339."),
        &|v| !is_rfc3339(v),
    );
}

const SPDX_IDS: &str = include_str!("spdx_license_ids.txt");

fn is_spdx_id(s: &str) -> bool {
    SPDX_IDS.lines().any(|id| id == s)
}

fn dl3054(doc: &Doc, schema: &[(String, LabelType)], out: &mut Vec<Failure>) {
    label_rule(
        doc,
        schema,
        out,
        "DL3054",
        Some(LabelType::Spdx),
        &|l| format!("Label `{l}` is not a valid SPDX identifier."),
        &|v| !is_spdx_id(v),
    );
}

fn dl3055(doc: &Doc, schema: &[(String, LabelType)], out: &mut Vec<Failure>) {
    label_rule(
        doc,
        schema,
        out,
        "DL3055",
        Some(LabelType::GitHash),
        &|l| format!("Label `{l}` is not a valid git hash."),
        &|v| {
            v.chars()
                .any(|c| !(c.is_ascii_digit() || ('a'..='f').contains(&c)))
                || (v.chars().count() != 40 && v.chars().count() != 7)
        },
    );
}

/// Data.SemVer `fromText`: `MAJOR.MINOR.PATCH[-pre][+build]`.
fn is_semver(s: &str) -> bool {
    let (core, rest) = match s.find(['-', '+']) {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    };
    let nums: Vec<&str> = core.split('.').collect();
    if nums.len() != 3
        || !nums.iter().all(|n| {
            !n.is_empty()
                && n.chars().all(|c| c.is_ascii_digit())
                && (*n == "0" || !n.starts_with('0'))
        })
    {
        return false;
    }
    let (pre, build) = match rest.find('+') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let ident_ok = |part: &str| {
        !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    if let Some(p) = pre.strip_prefix('-')
        && !p.split('.').all(ident_ok)
    {
        return false;
    }
    if !pre.is_empty() && !pre.starts_with('-') {
        return false;
    }
    build.is_none_or(|b| b.split('.').all(ident_ok))
}

fn dl3056(doc: &Doc, schema: &[(String, LabelType)], out: &mut Vec<Failure>) {
    label_rule(
        doc,
        schema,
        out,
        "DL3056",
        Some(LabelType::SemVer),
        &|l| format!("Label `{l}` does not conform to semantic versioning."),
        &|v| !is_semver(v),
    );
}

/// RFC 5322 addr-spec, the way `email-validate` judges it: a dot-atom or
/// quoted local part, an `@`, and a domain of dot-separated labels.
fn is_email(s: &str) -> bool {
    let Some(at) = s.rfind('@') else { return false };
    let (local, domain) = (&s[..at], &s[at + 1..]);
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    let atom_char = |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-/=?^_`{|}~".contains(c);
    let local_ok = if local.starts_with('"') && local.ends_with('"') && local.len() >= 2 {
        local[1..local.len() - 1]
            .chars()
            .all(|c| c != '"' || c == '\\')
    } else {
        local
            .split('.')
            .all(|p| !p.is_empty() && p.chars().all(atom_char))
    };
    let domain_ok = if domain.starts_with('[') && domain.ends_with(']') {
        domain.len() > 2
    } else {
        domain.split('.').all(|p| {
            !p.is_empty()
                && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !p.starts_with('-')
                && !p.ends_with('-')
        })
    };
    local_ok && domain_ok
}

fn dl3058(doc: &Doc, schema: &[(String, LabelType)], out: &mut Vec<Failure>) {
    label_rule(
        doc,
        schema,
        out,
        "DL3058",
        Some(LabelType::Email),
        &|l| format!("Label `{l}` is not a valid email format - must conform to RFC5322."),
        &|v| !is_email(v),
    );
}

// ---- DL3057 ---------------------------------------------------------------------

fn dl3057(doc: &Doc, out: &mut Vec<Failure>) {
    #[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct StageId {
        src: String,
        name: String,
        line: usize,
    }
    let mut current: Option<StageId> = None;
    let mut good: BTreeSet<StageId> = BTreeSet::new();
    let mut bad: BTreeSet<StageId> = BTreeSet::new();
    for ip in doc {
        match &ip.instruction {
            Instruction::From(from) => {
                let src = from.image.name.clone();
                let name = from.alias.clone().unwrap_or_else(|| src.clone());
                let id = StageId {
                    src: src.clone(),
                    name,
                    line: ip.line,
                };
                if current.is_some() && good.iter().any(|g| g.name == src) {
                    good.insert(id.clone());
                } else {
                    bad.insert(id.clone());
                }
                current = Some(id);
            }
            Instruction::Healthcheck(_) => {
                if let Some(cur) = &current {
                    // recurseGood: stages (transitively) built from this one
                    fn recurse(bad: &BTreeSet<StageId>, sid: &StageId) -> BTreeSet<StageId> {
                        let g1: BTreeSet<StageId> =
                            bad.iter().filter(|b| b.name == sid.src).cloned().collect();
                        if g1.is_empty() {
                            return g1;
                        }
                        let b1: BTreeSet<StageId> = bad.difference(&g1).cloned().collect();
                        let mut all = g1.clone();
                        for g in &g1 {
                            all.extend(recurse(&b1, g));
                        }
                        all
                    }
                    let now_good = recurse(&bad, cur);
                    good.extend(now_good.iter().cloned());
                    good.insert(cur.clone());
                    bad = bad.difference(&now_good).cloned().collect();
                    bad.remove(cur);
                }
            }
            _ => {}
        }
    }
    for s in bad {
        out.push(failure(
            "DL3057",
            Severity::Ignore,
            "`HEALTHCHECK` instruction missing.",
            s.line,
        ));
    }
}

// ---- DL3059 ---------------------------------------------------------------------

fn dl3059(doc: &Doc, out: &mut Vec<Failure>) {
    let mut state: Option<(RunFlags, usize)> = None;
    for ip in doc {
        match &ip.instruction {
            Instruction::Run { args, flags } => {
                let count = commands(args).len();
                let remember = match &state {
                    None => true,
                    Some((fl, prev)) => fl != flags || count > 2 || *prev > 2,
                };
                if remember {
                    state = Some((flags.clone(), count));
                } else {
                    out.push(failure(
                        "DL3059",
                        Severity::Info,
                        "Multiple consecutive `RUN` instructions. Consider consolidation.",
                        ip.line,
                    ));
                }
            }
            Instruction::Comment(_) => {}
            _ => state = None,
        }
    }
}

// ---- DL3060 ---------------------------------------------------------------------

fn dl3060(doc: &Doc, out: &mut Vec<Failure>) {
    struct Acc {
        current: BaseImage,
        active: BTreeMap<String, usize>,
        inactive: BTreeMap<usize, BaseImage>,
    }
    let mut acc: Option<Acc> = None;
    let install = |c: &Command| c.is_with_args("yarn", &["install"]);
    let clean = |c: &Command| c.is_with_args("yarn", &["cache", "clean"]);
    for ip in doc {
        match &ip.instruction {
            Instruction::From(from) => match &mut acc {
                Some(a) => {
                    a.current = from.clone();
                    a.active.insert(from.image.name.clone(), ip.line);
                }
                None => {
                    acc = Some(Acc {
                        current: from.clone(),
                        active: BTreeMap::from([(from.image.name.clone(), ip.line)]),
                        inactive: BTreeMap::new(),
                    });
                }
            },
            Instruction::Run { args, flags } => {
                let any_install = commands(args).iter().any(install);
                let any_clean = commands(args).iter().any(clean);
                let clean_before_install = matches!(
                    (find_command_index(args, clean), find_command_index(args, install)),
                    (Some(c), Some(i)) if c < i
                );
                let remember = any_install
                    && ((!any_clean && !flags.has_cache_or_tmpfs_mount_with(".cache/yarn"))
                        || (any_clean && clean_before_install));
                if remember {
                    match &mut acc {
                        Some(a) => {
                            a.inactive.insert(ip.line, a.current.clone());
                        }
                        None => {
                            acc = Some(Acc {
                                current: BaseImage::scratch(),
                                active: BTreeMap::new(),
                                inactive: BTreeMap::from([(ip.line, BaseImage::scratch())]),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(a) = acc {
        for (line, from) in &a.inactive {
            let used = from
                .alias
                .as_ref()
                .is_some_and(|als| a.active.contains_key(als));
            if *from == a.current || used {
                out.push(failure(
                    "DL3060",
                    Severity::Info,
                    "`yarn cache clean` missing after `yarn install` was run.",
                    *line,
                ));
            }
        }
    }
}

// ---- DL3061 - DL3067 ------------------------------------------------------------

fn dl3061(doc: &Doc, out: &mut Vec<Failure>) {
    let mut seen_from = false;
    for ip in doc {
        match &ip.instruction {
            Instruction::From(_) => seen_from = true,
            Instruction::Comment(_) | Instruction::Pragma(_) | Instruction::Arg { .. } => {}
            _ => {
                if !seen_from {
                    out.push(failure(
                        "DL3061",
                        Severity::Error,
                        "Invalid instruction order. Dockerfile must begin with `FROM`, `ARG` or comment.",
                        ip.line,
                    ));
                }
            }
        }
    }
}

fn dl3062(doc: &Doc, out: &mut Vec<Failure>) {
    let go_packages = |a: &Args| -> Vec<String> {
        let run: Vec<String> = commands(a)
            .iter()
            .filter(|c| c.is_any_with_args(&["go"], &["run"]))
            .flat_map(|c| {
                c.args_no_flags()
                    .into_iter()
                    .enumerate()
                    .filter(|(idx, arg)| *arg != "run" && *idx <= 1)
                    .map(|(_, arg)| arg.to_string())
                    .collect::<Vec<_>>()
            })
            .collect();
        if !run.is_empty() {
            return run;
        }
        commands(a)
            .iter()
            .filter(|c| c.is_any_with_args(&["go"], &["install", "get"]))
            .flat_map(|c| {
                c.args_no_flags()
                    .into_iter()
                    .filter(|arg| !["install", "get", "tool"].contains(arg))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let ok = |p: &str| {
        let local = p == "." || p.starts_with('/') || p.starts_with('.');
        let tags = ["latest", "none"]
            .iter()
            .any(|t| p.ends_with(&format!("@{t}")));
        local || (p.contains('@') && !tags)
    };
    simple(
        doc,
        out,
        "DL3062",
        Severity::Warning,
        "Pin versions in go. Instead of `go install <package>` use `go install <package>@<version>`",
        |i| run_args(i).is_none_or(|(a, _)| go_packages(a).iter().all(|p| ok(p))),
    );
}

fn dl3063(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3063",
        Severity::Warning,
        "stage name should not be a reserved word",
        |i| match i {
            Instruction::From(BaseImage { alias: Some(a), .. }) => a != "scratch" && a != "context",
            _ => true,
        },
    );
}

const SENSITIVE_NAMES: &[&str] = &[
    "ACCESS_TOKEN",
    "APPLICATION_KEY",
    "APP_SECRET",
    "AUTH_TOKEN",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "BITTREX_API_KEY",
    "BITTREX_API_SECRET",
    "CF_PASSWORD",
    "CF_USERNAME",
    "CIRCLE_TOKEN",
    "CI_DEPLOY_PASSWORD",
    "CI_DEPLOY_USER",
    "DOCKERHUB_PASSWORD",
    "DOCKER_EMAIL",
    "DOCKER_PASSWORD",
    "DOCKER_USERNAME",
    "FACEBOOK_ACCESS_TOKEN",
    "FACEBOOK_APP_ID",
    "FACEBOOK_APP_SECRET",
    "FIREBASE_API_TOKEN",
    "FIREBASE_TOKEN",
    "FOSSA_API_KEY",
    "GH_ENTERPRISE_TOKEN",
    "GH_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
    "GITHUB_TOKEN",
    "HEROKU_API_KEY",
    "HEROKU_API_USER",
    "NPM_AUTH_TOKEN",
    "NPM_TOKEN",
    "OKTA_AUTHN_GROUPID",
    "OKTA_CLIENT_ORGURL",
    "OKTA_CLIENT_TOKEN",
    "OKTA_OAUTH2_CLIENTID",
    "OKTA_OAUTH2_CLIENTSECRET",
    "OPENAI_API_KEY",
    "OS_PASSWORD",
    "OS_USERNAME",
    "POSTGRES_PASSWORD",
    "SLACK_TOKEN",
    "STRIPE_API_KEY",
    "STRIPE_DEVICE_NAME",
    "TRAVIS_OS_NAME",
    "TRAVIS_SECURE_ENV_VARS",
    "TRAVIS_SUDO",
    "VAULT_CLIENT_KEY",
    "VAULT_TOKEN",
];

const SUSPICIOUS: &[&str] = &[
    "api_key",
    "client_key",
    "password",
    "private",
    "secret",
    "token",
    "username",
];

fn dl3064(doc: &Doc, out: &mut Vec<Failure>) {
    let sensitive = |name: &str| {
        SENSITIVE_NAMES.contains(&name.to_uppercase().as_str())
            || SUSPICIOUS.iter().any(|s| name.to_lowercase().contains(s))
    };
    simple(
        doc,
        out,
        "DL3064",
        Severity::Warning,
        "Potentially sensitive data should not be used in the `ARG` or `ENV` commands",
        |i| match i {
            Instruction::Arg { name, .. } => !sensitive(name),
            Instruction::Env(pairs) => !pairs.iter().any(|(k, _)| sensitive(k)),
            _ => true,
        },
    );
}

fn dl3065(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3065",
        Severity::Warning,
        "Setting `FROM --platform` to predefined `$TARGETPLATFORM` in is redundant as this is the default behavior",
        |i| match i {
            Instruction::From(BaseImage {
                platform: Some(p), ..
            }) => p != "$TARGETPLATFORM" && p != "${TARGETPLATFORM}",
            _ => true,
        },
    );
}

fn dl3066(doc: &Doc, out: &mut Vec<Failure>) {
    let mut args: BTreeSet<String> = BTreeSet::new();
    for ip in doc {
        match &ip.instruction {
            Instruction::User(u) => {
                let uid = u.split(':').next().unwrap_or("");
                let numeric = uid.chars().all(|c| c.is_ascii_digit());
                let defined = args
                    .iter()
                    .any(|v| u.contains(&format!("${{{v}}}")) || u.contains(&format!("${v}")));
                if !numeric && !defined {
                    out.push(failure(
                        "DL3066",
                        Severity::Info,
                        "Non-numeric user-id may not be resolvable by host system",
                        ip.line,
                    ));
                }
            }
            Instruction::Arg { name, .. } => {
                args.insert(name.clone());
            }
            _ => {}
        }
    }
}

fn dl3067(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL3067",
        Severity::Warning,
        "Do not copy an entire filesystem from another stage",
        |i| match i {
            Instruction::Copy {
                sources,
                target,
                flags,
            } if flags.from.is_some() => {
                !(sources.iter().any(|s| drop_quotes(s) == "/") && drop_quotes(target) == "/")
            }
            _ => true,
        },
    );
}

// ---- DL4000 - DL4006 ------------------------------------------------------------

fn dl4000(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL4000",
        Severity::Error,
        "MAINTAINER is deprecated",
        |i| !matches!(i, Instruction::Maintainer(_)),
    );
}

fn dl4001(doc: &Doc, out: &mut Vec<Failure>) {
    let mut seen: BTreeSet<u8> = BTreeSet::new();
    for ip in doc {
        match &ip.instruction {
            Instruction::From(_) => seen.clear(),
            Instruction::Run { args, .. } => {
                let new: BTreeSet<u8> = command_names(args)
                    .into_iter()
                    .filter_map(|n| match n {
                        "curl" => Some(0),
                        "wget" => Some(1),
                        _ => None,
                    })
                    .collect();
                seen.extend(new.iter().copied());
                if !new.is_empty() && seen.len() >= 2 {
                    out.push(failure(
                        "DL4001",
                        Severity::Warning,
                        "Either use Wget or Curl but not both",
                        ip.line,
                    ));
                }
            }
            _ => {}
        }
    }
}

fn dl4003(doc: &Doc, out: &mut Vec<Failure>) {
    let mut has = false;
    for ip in doc {
        match &ip.instruction {
            Instruction::From(_) => has = false,
            Instruction::Cmd(_) => {
                if has {
                    out.push(failure(
                        "DL4003",
                        Severity::Warning,
                        "Multiple `CMD` instructions found. If you list more than one `CMD` then only the last `CMD` will take effect",
                        ip.line,
                    ));
                } else {
                    has = true;
                }
            }
            _ => {}
        }
    }
}

fn dl4004(doc: &Doc, out: &mut Vec<Failure>) {
    let mut has = false;
    for ip in doc {
        match &ip.instruction {
            Instruction::From(_) => has = false,
            Instruction::Entrypoint(_) => {
                if has {
                    out.push(failure(
                        "DL4004",
                        Severity::Error,
                        "Multiple `ENTRYPOINT` instructions found. If you list more than one `ENTRYPOINT` then only the last `ENTRYPOINT` will take effect",
                        ip.line,
                    ));
                } else {
                    has = true;
                }
            }
            _ => {}
        }
    }
}

fn dl4005(doc: &Doc, out: &mut Vec<Failure>) {
    simple(
        doc,
        out,
        "DL4005",
        Severity::Warning,
        "Use SHELL to change the default shell",
        |i| run_args(i).is_none_or(|(a, _)| no_commands(a, |c| c.is_with_args("ln", &["/bin/sh"]))),
    );
}

fn dl4006(doc: &Doc, out: &mut Vec<Failure>) {
    const NON_POSIX: &[&str] = &["pwsh", "powershell", "cmd"];
    const VALID_SHELLS: &[&str] = &["/bin/bash", "/bin/zsh", "/bin/ash", "bash", "zsh", "ash"];
    let mut pipefail = false;
    for ip in doc {
        match &ip.instruction {
            Instruction::From(_) => pipefail = false,
            Instruction::Shell(args) => {
                if NON_POSIX.iter().any(|s| args.shell.original.starts_with(s)) {
                    pipefail = true;
                } else {
                    pipefail = commands(args).iter().any(|c| {
                        VALID_SHELLS.contains(&c.name.as_str())
                            && c.has_flag("o")
                            && c.has_arg("pipefail")
                    });
                }
            }
            Instruction::Run { args, .. } if !pipefail && args.shell.has_pipes => {
                out.push(failure(
                        "DL4006",
                        Severity::Warning,
                        "Set the SHELL option -o pipefail before RUN with a pipe in it. If you are using /bin/sh in an alpine image or if your shell is symlinked to busybox then consider explicitly setting your SHELL to /bin/ash, or disable this check",
                        ip.line,
                    ));
            }
            _ => {}
        }
    }
}
