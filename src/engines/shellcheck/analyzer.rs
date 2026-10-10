//! The analyzer (`ShellCheck.Analyzer`): runs the checks over a parsed
//! script.

use super::analyzerlib::{filter_by_annotation, make_parameters};
use super::ast::{Id, IntMap, Token, do_analysis};
use super::interface::{Position, Shell, TokenComment};

/// `AnalysisSpec`.
pub struct AnalysisSpec<'a> {
    pub script: &'a Token,
    pub shell_type: Option<Shell>,
    pub fallback_shell: Option<Shell>,
    pub check_sourced: bool,
    pub optional_checks: Vec<String>,
    pub extended_analysis: Option<bool>,
    pub token_positions: &'a IntMap<Id, (Position, Position)>,
}

/// `analyzeScript`.
pub fn analyze_script(spec: &AnalysisSpec<'_>) -> Vec<TokenComment> {
    let params = make_parameters(spec);
    let root = params.root_node;
    // The per-script checks (Analytics; Commands, ControlFlow, Custom and
    // ShellSupport have none that do anything), then one traversal running
    // the per-token checks (Commands and ShellSupport).
    let mut comments = super::analytics3::run(&params, root, &spec.optional_checks);
    let commands = super::commands::Commands::new(&spec.optional_checks);
    do_analysis(root, &mut |t| {
        commands.check(&params, t, &mut comments);
        super::shellsupport::check(&params, t, &mut comments);
    });
    let mut unique: Vec<TokenComment> = Vec::new();
    for c in comments {
        if !unique.contains(&c) {
            unique.push(c);
        }
    }
    filter_by_annotation(spec, &params, unique)
}
