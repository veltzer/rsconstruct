//! The `action` rule: `uses:` specs, popular actions' inputs, and the
//! metadata of local actions.

use std::path::Path;

use super::{Rule, RuleBase};
use crate::actionlint::action_metadata::{
    ActionMetadata, ActionMetadataRuns, LocalActionsCache, is_outdated_popular_action,
    popular_action,
};
use crate::actionlint::ast::{Exec, ExecAction, Pos, Step};
use crate::actionlint::parse::sorted_quotes;
use crate::actionlint::yaml::go_quote;

const BRANDING_COLORS: &[&str] = &[
    "white",
    "black",
    "yellow",
    "blue",
    "green",
    "orange",
    "red",
    "purple",
    "gray-dark",
];

const BRANDING_ICONS: &[&str] = &[
    "activity",
    "airplay",
    "alert-circle",
    "alert-octagon",
    "alert-triangle",
    "align-center",
    "align-justify",
    "align-left",
    "align-right",
    "anchor",
    "aperture",
    "archive",
    "arrow-down-circle",
    "arrow-down-left",
    "arrow-down-right",
    "arrow-down",
    "arrow-left-circle",
    "arrow-left",
    "arrow-right-circle",
    "arrow-right",
    "arrow-up-circle",
    "arrow-up-left",
    "arrow-up-right",
    "arrow-up",
    "at-sign",
    "award",
    "bar-chart-2",
    "bar-chart",
    "battery-charging",
    "battery",
    "bell-off",
    "bell",
    "bluetooth",
    "bold",
    "book-open",
    "book",
    "bookmark",
    "box",
    "briefcase",
    "calendar",
    "camera-off",
    "camera",
    "cast",
    "check-circle",
    "check-square",
    "check",
    "chevron-down",
    "chevron-left",
    "chevron-right",
    "chevron-up",
    "chevrons-down",
    "chevrons-left",
    "chevrons-right",
    "chevrons-up",
    "circle",
    "clipboard",
    "clock",
    "cloud-drizzle",
    "cloud-lightning",
    "cloud-off",
    "cloud-rain",
    "cloud-snow",
    "cloud",
    "code",
    "command",
    "compass",
    "copy",
    "corner-down-left",
    "corner-down-right",
    "corner-left-down",
    "corner-left-up",
    "corner-right-down",
    "corner-right-up",
    "corner-up-left",
    "corner-up-right",
    "cpu",
    "credit-card",
    "crop",
    "crosshair",
    "database",
    "delete",
    "disc",
    "dollar-sign",
    "download-cloud",
    "download",
    "droplet",
    "edit-2",
    "edit-3",
    "edit",
    "external-link",
    "eye-off",
    "eye",
    "fast-forward",
    "feather",
    "file-minus",
    "file-plus",
    "file-text",
    "file",
    "film",
    "filter",
    "flag",
    "folder-minus",
    "folder-plus",
    "folder",
    "gift",
    "git-branch",
    "git-commit",
    "git-merge",
    "git-pull-request",
    "globe",
    "grid",
    "hard-drive",
    "hash",
    "headphones",
    "heart",
    "help-circle",
    "home",
    "image",
    "inbox",
    "info",
    "italic",
    "layers",
    "layout",
    "life-buoy",
    "link-2",
    "link",
    "list",
    "loader",
    "lock",
    "log-in",
    "log-out",
    "mail",
    "map-pin",
    "map",
    "maximize-2",
    "maximize",
    "menu",
    "message-circle",
    "message-square",
    "mic-off",
    "mic",
    "minimize-2",
    "minimize",
    "minus-circle",
    "minus-square",
    "minus",
    "monitor",
    "moon",
    "more-horizontal",
    "more-vertical",
    "move",
    "music",
    "navigation-2",
    "navigation",
    "octagon",
    "package",
    "paperclip",
    "pause-circle",
    "pause",
    "percent",
    "phone-call",
    "phone-forwarded",
    "phone-incoming",
    "phone-missed",
    "phone-off",
    "phone-outgoing",
    "phone",
    "pie-chart",
    "play-circle",
    "play",
    "plus-circle",
    "plus-square",
    "plus",
    "pocket",
    "power",
    "printer",
    "radio",
    "refresh-ccw",
    "refresh-cw",
    "repeat",
    "rewind",
    "rotate-ccw",
    "rotate-cw",
    "rss",
    "save",
    "scissors",
    "search",
    "send",
    "server",
    "settings",
    "share-2",
    "share",
    "shield-off",
    "shield",
    "shopping-bag",
    "shopping-cart",
    "shuffle",
    "sidebar",
    "skip-back",
    "skip-forward",
    "slash",
    "sliders",
    "smartphone",
    "speaker",
    "square",
    "star",
    "stop-circle",
    "sun",
    "sunrise",
    "sunset",
    "table",
    "tablet",
    "tag",
    "target",
    "terminal",
    "thermometer",
    "thumbs-down",
    "thumbs-up",
    "toggle-left",
    "toggle-right",
    "trash-2",
    "trash",
    "trending-down",
    "trending-up",
    "triangle",
    "truck",
    "tv",
    "type",
    "umbrella",
    "underline",
    "unlock",
    "upload-cloud",
    "upload",
    "user-check",
    "user-minus",
    "user-plus",
    "user-x",
    "user",
    "users",
    "video-off",
    "video",
    "voicemail",
    "volume-1",
    "volume-2",
    "volume-x",
    "volume",
    "watch",
    "wifi-off",
    "wifi",
    "wind",
    "x-circle",
    "x-square",
    "x",
    "zap-off",
    "zap",
    "zoom-in",
    "zoom-out",
];

fn is_image_on_docker_registry(image: &str) -> bool {
    ["docker://", "gcr.io/", "pkg.dev/", "ghcr.io/", "docker.io/"]
        .iter()
        .any(|p| image.starts_with(p))
}

pub struct RuleAction<'a> {
    base: RuleBase,
    cache: &'a LocalActionsCache,
}

impl<'a> RuleAction<'a> {
    pub const fn new(cache: &'a LocalActionsCache) -> Self {
        Self {
            base: RuleBase::new("action"),
            cache,
        }
    }

    /// `{owner}/{repo}@{ref}` or `{owner}/{repo}/{path}@{ref}`.
    fn check_repo_action(&mut self, spec: &str, exec: &ExecAction) {
        let uses_pos = exec.uses.as_ref().map_or_else(Pos::default, |u| u.pos);
        let Some(at) = spec.find('@') else {
            self.invalid_action_format(uses_pos, spec, "ref is missing");
            return;
        };
        let reference = &spec[at + 1..];
        let s = &spec[..at];
        let Some(slash) = s.find('/') else {
            self.invalid_action_format(uses_pos, spec, "owner is missing");
            return;
        };
        let owner = &s[..slash];
        let s = &s[slash + 1..];
        let repo = s.find('/').map_or(s, |i| &s[..i]);
        if owner.is_empty() || repo.is_empty() || reference.is_empty() {
            self.invalid_action_format(
                uses_pos,
                spec,
                "owner and repo and ref should not be empty",
            );
        }
        let Some(meta) = popular_action(spec) else {
            if is_outdated_popular_action(spec) {
                self.base.error(
                    uses_pos,
                    format!(
                        "the runner of {} action is too old to run on GitHub Actions. update the action's version to fix this issue",
                        go_quote(spec)
                    ),
                );
            }
            return;
        };
        if meta.skip_inputs {
            return;
        }
        let described = go_quote(spec);
        self.check_action(&meta, exec, &described);
    }

    fn invalid_action_format(&mut self, pos: Pos, spec: &str, why: &str) {
        self.base.error(
            pos,
            format!(
                "specifying action {} in invalid format because {why}. available formats are \"{{owner}}/{{repo}}@{{ref}}\" or \"{{owner}}/{{repo}}/{{path}}@{{ref}}\"",
                go_quote(spec)
            ),
        );
    }

    fn missing_runs_prop(&mut self, pos: Pos, prop: &str, ty: &str, action: &str, path: &str) {
        self.base.error(
            pos,
            format!(
                "{} is required in \"runs\" section because {} is a {ty} action. the action is defined at {}",
                go_quote(prop),
                go_quote(action),
                go_quote(path)
            ),
        );
    }

    fn check_invalid_runs_props(
        &mut self,
        pos: Pos,
        r: &ActionMetadataRuns,
        ty: &str,
        action: &str,
        path: &str,
        props: &[&str],
    ) {
        for prop in props {
            let invalid = match *prop {
                "main" => !r.main.is_empty(),
                "pre" => !r.pre.is_empty(),
                "pre-if" => !r.pre_if.is_empty(),
                "post" => !r.post.is_empty(),
                "post-if" => !r.post_if.is_empty(),
                "steps" => r.steps.is_some_and(|n| n > 0),
                "image" => !r.image.is_empty(),
                "pre-entrypoint" => !r.pre_entrypoint.is_empty(),
                "entrypoint" => !r.entrypoint.is_empty(),
                "post-entrypoint" => !r.post_entrypoint.is_empty(),
                "args" => r.args,
                "env" => r.env,
                _ => false,
            };
            if invalid {
                self.base.error(
                    pos,
                    format!(
                        "{} is not allowed in \"runs\" section because {} is a {ty} action. the action is defined at {}",
                        go_quote(prop),
                        go_quote(action),
                        go_quote(path)
                    ),
                );
            }
        }
    }

    fn check_runs_file_exists(&mut self, file: &str, dir: &Path, prop: &str, name: &str, pos: Pos) {
        if file.is_empty() {
            return;
        }
        if !dir.join(file).exists() {
            self.base.error(
                pos,
                format!(
                    "file {} does not exist in {}. it is specified at {} key in \"runs\" section in {} action",
                    go_quote(file),
                    go_quote(&dir.display().to_string()),
                    go_quote(prop),
                    go_quote(name)
                ),
            );
        }
    }

    fn check_local_docker_action_runs(
        &mut self,
        r: &ActionMetadataRuns,
        dir: &Path,
        name: &str,
        pos: Pos,
    ) {
        let dir_s = dir.display().to_string();
        if r.image.is_empty() {
            self.missing_runs_prop(pos, "image", "Docker", name, &dir_s);
        } else if !is_image_on_docker_registry(&r.image) {
            self.check_runs_file_exists(&r.image, dir, "image", name, pos);
            if Path::new(&r.image).file_name().and_then(|f| f.to_str()) != Some("Dockerfile") {
                self.base.error(
                    pos,
                    format!(
                        "the local file {} referenced from \"image\" key must be named \"Dockerfile\" in {} action. the action is defined at {}",
                        go_quote(&r.image),
                        go_quote(name),
                        go_quote(&dir_s)
                    ),
                );
            }
        }
        self.check_runs_file_exists(&r.pre_entrypoint, dir, "pre-entrypoint", name, pos);
        self.check_runs_file_exists(&r.entrypoint, dir, "entrypoint", name, pos);
        self.check_runs_file_exists(&r.post_entrypoint, dir, "post-entrypoint", name, pos);
        self.check_invalid_runs_props(
            pos,
            r,
            "Docker",
            name,
            &dir_s,
            &["main", "pre", "pre-if", "post", "post-if", "steps"],
        );
    }

    fn check_local_composite_action_runs(
        &mut self,
        r: &ActionMetadataRuns,
        dir: &Path,
        name: &str,
        pos: Pos,
    ) {
        let dir_s = dir.display().to_string();
        if r.steps.is_none() {
            self.missing_runs_prop(pos, "steps", "Composite", name, &dir_s);
        }
        self.check_invalid_runs_props(
            pos,
            r,
            "Composite",
            name,
            &dir_s,
            &[
                "main",
                "pre",
                "pre-if",
                "post",
                "post-if",
                "image",
                "pre-entrypoint",
                "entrypoint",
                "post-entrypoint",
                "args",
                "env",
            ],
        );
    }

    fn check_local_javascript_action_runs(
        &mut self,
        r: &ActionMetadataRuns,
        dir: &Path,
        name: &str,
        pos: Pos,
    ) {
        let dir_s = dir.display().to_string();
        if r.main.is_empty() {
            self.missing_runs_prop(pos, "main", "JavaScript", name, &dir_s);
        } else {
            self.check_runs_file_exists(&r.main, dir, "main", name, pos);
        }
        self.check_runs_file_exists(&r.pre, dir, "pre", name, pos);
        if r.pre.is_empty() && !r.pre_if.is_empty() {
            self.base.error(
                pos,
                format!(
                    "\"pre\" is required when \"pre-if\" is specified in \"runs\" section in {} action. the action is defined at {}",
                    go_quote(name),
                    go_quote(&dir_s)
                ),
            );
        }
        self.check_runs_file_exists(&r.post, dir, "post", name, pos);
        if r.post.is_empty() && !r.post_if.is_empty() {
            self.base.error(
                pos,
                format!(
                    "\"post\" is required when \"post-if\" is specified in \"runs\" section in {} action. the action is defined at {}",
                    go_quote(name),
                    go_quote(&dir_s)
                ),
            );
        }
        self.check_invalid_runs_props(
            pos,
            r,
            "JavaScript",
            name,
            &dir_s,
            &[
                "steps",
                "image",
                "pre-entrypoint",
                "entrypoint",
                "post-entrypoint",
                "args",
                "env",
            ],
        );
    }

    fn check_local_action_inputs(&mut self, meta: &ActionMetadata, pos: Pos) {
        for i in meta.inputs.values() {
            if i.deprecated && i.deprecation_message.is_empty() {
                self.base.error(
                    pos,
                    format!(
                        "input {} is deprecated but \"deprecationMessage\" is empty in metadata of {} action at {}",
                        go_quote(&i.name),
                        go_quote(&meta.name),
                        go_quote(&meta.path().display().to_string())
                    ),
                );
            }
        }
    }

    fn check_local_action_runs(&mut self, meta: &ActionMetadata, pos: Pos) {
        let r = &meta.runs;
        let dir = meta.dir();
        match r.using.as_str() {
            "" => self.base.error(
                pos,
                format!(
                    "\"runs.using\" is missing in local action {} defined at {}",
                    go_quote(&meta.name),
                    go_quote(&dir.display().to_string())
                ),
            ),
            "docker" => self.check_local_docker_action_runs(r, dir, &meta.name, pos),
            "composite" => self.check_local_composite_action_runs(r, dir, &meta.name, pos),
            "node20" | "node24" => self.check_local_javascript_action_runs(r, dir, &meta.name, pos),
            other => {
                self.base.error(
                    pos,
                    format!(
                        "invalid runner name {} at runs.using in {} action defined at {}. valid runners are \"composite\", \"docker\", \"node20\", and \"node24\". see https://docs.github.com/en/actions/creating-actions/metadata-syntax-for-github-actions#runs",
                        go_quote(other),
                        go_quote(&meta.name),
                        go_quote(&dir.display().to_string())
                    ),
                );
                if other.starts_with("node") {
                    self.check_local_javascript_action_runs(r, dir, &meta.name, pos);
                }
            }
        }
    }

    fn check_docker_action(&mut self, uri: &str, exec: &ExecAction) {
        let pos = exec.uses.as_ref().map_or_else(Pos::default, |u| u.pos);
        let mut uri = uri;
        let mut tag = "";
        let mut tag_exists = false;
        if let Some(idx) = uri["docker://".len()..].find(':') {
            let idx = idx + "docker://".len();
            if idx < uri.len() {
                tag = &uri[idx + 1..];
                uri = &uri[..idx];
                tag_exists = true;
            }
        }
        if url_parse_fails(uri) {
            self.base.error(
                pos,
                format!(
                    "URI for Docker container {} is invalid: parse {}: invalid URI for request (tag={tag})",
                    go_quote(uri),
                    go_quote(uri)
                ),
            );
        }
        if tag_exists && tag.is_empty() {
            self.base.error(
                pos,
                format!(
                    "tag of Docker action should not be empty: {}",
                    go_quote(uri)
                ),
            );
        }
    }

    fn check_local_action_metadata(&mut self, meta: &ActionMetadata, action: &ExecAction) {
        let pos = action.uses.as_ref().map_or_else(Pos::default, |u| u.pos);
        let path = meta.path().display().to_string();
        if meta.name.is_empty() {
            self.base.error(
                pos,
                format!("name is required in action metadata {}", go_quote(&path)),
            );
        }
        if meta.description.is_empty() {
            self.base.error(
                pos,
                format!(
                    "description is required in metadata of {} action at {}",
                    go_quote(&meta.name),
                    go_quote(&path)
                ),
            );
        }
        if !meta.branding.icon.is_empty()
            && !BRANDING_ICONS.contains(&meta.branding.icon.to_lowercase().as_str())
        {
            self.base.error(
                pos,
                format!(
                    "incorrect icon name {} at branding.icon in metadata of {} action at {}. see the official document to know the exhaustive list of supported icons: https://docs.github.com/en/actions/creating-actions/metadata-syntax-for-github-actions#brandingicon",
                    go_quote(&meta.branding.icon),
                    go_quote(&meta.name),
                    go_quote(&path)
                ),
            );
        }
        if !meta.branding.color.is_empty()
            && !BRANDING_COLORS.contains(&meta.branding.color.to_lowercase().as_str())
        {
            self.base.error(
                pos,
                format!(
                    "incorrect color {} at branding.icon in metadata of {} action at {}. see the official document to know the exhaustive list of supported colors: https://docs.github.com/en/actions/creating-actions/metadata-syntax-for-github-actions#brandingcolor",
                    go_quote(&meta.branding.color),
                    go_quote(&meta.name),
                    go_quote(&path)
                ),
            );
        }
        self.check_local_action_inputs(meta, pos);
        self.check_local_action_runs(meta, pos);
    }

    fn check_local_action(&mut self, spec: &str, action: &ExecAction) {
        let pos = action.uses.as_ref().map_or_else(Pos::default, |u| u.pos);
        let (meta, cached) = match self.cache.find_metadata(spec) {
            Ok(found) => found,
            Err(e) => {
                self.base.error(pos, e);
                return;
            }
        };
        let Some(meta) = meta else {
            return;
        };
        if !cached {
            self.check_local_action_metadata(&meta, action);
        }
        let described = format!("{} defined at {}", go_quote(&meta.name), go_quote(spec));
        self.check_action(&meta, action, &described);
    }

    fn check_action(&mut self, meta: &ActionMetadata, exec: &ExecAction, described: &str) {
        for (id, i) in &exec.inputs {
            match meta.inputs.get(id) {
                None => {
                    let names: Vec<&str> = meta.inputs.values().map(|i| i.name.as_str()).collect();
                    self.base.error(
                        i.name.pos,
                        format!(
                            "input {} is not defined in action {described}. available inputs are {}",
                            go_quote(&i.name.value),
                            sorted_quotes(&names)
                        ),
                    );
                }
                Some(m) if m.deprecated && !m.required => {
                    let mut msg = format!(
                        "avoid using deprecated input {} in action {described}",
                        go_quote(&i.name.value)
                    );
                    let d = collapse_newlines(m.deprecation_message.trim_end_matches(['.', ' ']));
                    if !d.is_empty() {
                        msg.push_str(": ");
                        msg.push_str(&d);
                    }
                    self.base.error(i.name.pos, msg);
                }
                Some(_) => {}
            }
        }
        for (id, i) in &meta.inputs {
            if i.required && !exec.inputs.contains_key(id) {
                let required: Vec<&str> = meta
                    .inputs
                    .values()
                    .filter(|i| i.required)
                    .map(|i| i.name.as_str())
                    .collect();
                let pos = exec.uses.as_ref().map_or_else(Pos::default, |u| u.pos);
                self.base.error(
                    pos,
                    format!(
                        "missing input {} which is required by action {described}. all required inputs are {}",
                        go_quote(&i.name),
                        sorted_quotes(&required)
                    ),
                );
            }
        }
    }
}

/// `\s*\r?\n\s*` collapsed to one space.
fn collapse_newlines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_ws = String::new();
    for c in s.chars() {
        if c.is_whitespace() {
            pending_ws.push(c);
            continue;
        }
        if !pending_ws.is_empty() {
            if pending_ws.contains('\n') {
                out.push(' ');
            } else {
                out.push_str(&pending_ws);
            }
            pending_ws.clear();
        }
        out.push(c);
    }
    if !pending_ws.is_empty() {
        if pending_ws.contains('\n') {
            out.push(' ');
        } else {
            out.push_str(&pending_ws);
        }
    }
    out
}

/// Whether Go's `url.Parse` would reject the URI: a control character, or a
/// malformed scheme.
fn url_parse_fails(uri: &str) -> bool {
    uri.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}')
}

impl Rule for RuleAction<'_> {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_step(&mut self, step: &Step) {
        let Some(Exec::Action(e)) = &step.exec else {
            return;
        };
        let Some(uses) = &e.uses else {
            return;
        };
        if uses.contains_expression() {
            return;
        }
        let spec = uses.value.clone();
        if spec.starts_with("./") {
            self.check_local_action(&spec, e);
            return;
        }
        if spec.starts_with("docker://") {
            self.check_docker_action(&spec, e);
            return;
        }
        self.check_repo_action(&spec, e);
    }
}
