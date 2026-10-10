//! Data flow analysis on the control flow graph (`ShellCheck.CFGAnalysis`).
//!
//! An iterative data flow analysis in which function and subshell
//! invocations run their own flow, with a stack of frames, per-frame
//! dependency tracking, a cache keyed on those dependencies, and versioned
//! maps that make equal states cheap to recognize. The algorithm, its
//! caching and its versioning are `ShellCheck`'s: the cache can change which
//! states are merged, so it is part of the semantics.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use imbl::GenericOrdMap;
use imbl::ordmap::DiffItem;
use imbl::shared_ptr::RcK;

use super::ast::{Id, IntMap, Token};
use super::cfg::{
    CfEdge, CfEffect, CfNode, CfStringPart, CfValue, CfVariableProp, CfgParameters, Graph,
    IdTagged, Node, PropSet, Scope, build_graph,
};
use super::data;

const ITERATION_COUNT: u64 = 1_000_000;
const FALLBACK_THRESHOLD: u64 = 10_000;
const CACHE_ENTRIES: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(
    clippy::enum_variant_names,
    reason = "the variant names are ShellCheck's SpaceStatus constructors"
)]
pub enum SpaceStatus {
    SpaceStatusEmpty,
    SpaceStatusClean,
    SpaceStatusDirty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(
    clippy::enum_variant_names,
    reason = "the variant names are ShellCheck's NumericalStatus constructors"
)]
pub enum NumericalStatus {
    NumericalStatusUnknown,
    NumericalStatusEmpty,
    NumericalStatusMaybe,
    NumericalStatusDefinitely,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VariableValue {
    pub literal_value: Option<String>,
    pub space_status: SpaceStatus,
    pub numerical_status: NumericalStatus,
}

/// `Set (Set CFVariableProp)`: which of the sixteen `PropSet`s are
/// members, one bit each, so a variable state allocates nothing for its
/// properties. Ordered as the `Data.Set` is, by its members in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VariableProperties(u16);

impl VariableProperties {
    const fn single(set: PropSet) -> Self {
        Self(1 << set.bits())
    }

    /// The members, in no particular order.
    fn members(self) -> impl Iterator<Item = PropSet> {
        (0..PropSet::COUNT as u8)
            .filter(move |b| self.0 & (1 << b) != 0)
            .map(PropSet::from_bits)
    }

    /// Whether any member has the property.
    pub fn any_contains(self, p: CfVariableProp) -> bool {
        self.members().any(|s| s.contains(p))
    }

    /// Whether every member has the property.
    pub fn all_contain(self, p: CfVariableProp) -> bool {
        self.members().all(|s| s.contains(p))
    }

    const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// `S.map f`.
    fn map(self, f: impl Fn(PropSet) -> PropSet) -> Self {
        Self(self.members().fold(0, |acc, s| acc | Self::single(f(s)).0))
    }
}

impl Ord for VariableProperties {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let sorted = |v: Self| {
            let mut members: Vec<PropSet> = v.members().collect();
            members.sort();
            members
        };
        sorted(*self).cmp(&sorted(*other))
    }
}

impl PartialOrd for VariableProperties {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VariableState {
    pub variable_value: VariableValue,
    pub variable_properties: VariableProperties,
}

/// The program state exposed to checks: `ShellCheck`'s `ProgramState`, read
/// through the internal state it is built from. `ShellCheck` builds the
/// external map lazily, only for the nodes a check asks about; built
/// eagerly for every node it costs gigabytes on a large script, so the
/// variables are looked up on demand instead.
#[derive(Clone, Debug)]
pub struct ProgramState(InternalState);

impl ProgramState {
    /// A variable in scope (prefix, then local, then global), its literal
    /// value censored as `ShellCheck`'s `internalToExternal` does.
    pub fn variable(&self, name: &str) -> Option<VariableState> {
        let (mut v, _) = get_variable_with_scope(&self.0, name)?;
        v.variable_value.literal_value = None;
        Some(v)
    }

    pub fn exit_codes(&self) -> BTreeSet<Id> {
        self.0
            .exit_codes
            .as_ref()
            .map(|r| (**r).clone())
            .unwrap_or_default()
    }

    pub fn state_is_reachable(&self) -> bool {
        self.0.is_reachable.unwrap_or(true)
    }
}

/// `CFGAnalysis`.
pub struct CfgAnalysis {
    pub graph: Graph,
    pub token_to_range: IntMap<Id, (Node, Node)>,
    pub token_to_nodes: IntMap<Id, BTreeSet<Node>>,
    pub post_dominators: Vec<BTreeSet<Node>>,
    pub node_to_data: BTreeMap<Node, (ProgramState, ProgramState)>,
}

/// `getIncomingState`.
pub fn get_incoming_state(analysis: &CfgAnalysis, id: Id) -> Option<&ProgramState> {
    let (start, _) = analysis.token_to_range.get(&id)?;
    analysis.node_to_data.get(start).map(|(a, _)| a)
}

/// `doesPostDominate`: whether `target` always runs after `base`.
pub fn does_post_dominate(analysis: &CfgAnalysis, target: Id, base: Id) -> bool {
    let Some((_, base_end)) = analysis.token_to_range.get(&base) else {
        return false;
    };
    let Some((target_start, _)) = analysis.token_to_range.get(&target) else {
        return false;
    };
    analysis
        .post_dominators
        .get(*base_end)
        .is_some_and(|s| s.contains(target_start))
}

fn variable_may_have_state(
    state: &ProgramState,
    var: &str,
    property: CfVariableProp,
) -> Option<bool> {
    let value = state.variable(var)?;
    Some(value.variable_properties.any_contains(property))
}

/// `variableMayBeDeclaredInteger`.
pub fn variable_may_be_declared_integer(state: &ProgramState, var: &str) -> Option<bool> {
    variable_may_have_state(state, var, CfVariableProp::CFVPInteger)
}

/// `variableMayBeAssignedInteger`.
pub fn variable_may_be_assigned_integer(state: &ProgramState, var: &str) -> Option<bool> {
    let value = state.variable(var)?;
    Some(value.variable_value.numerical_status >= NumericalStatus::NumericalStatusMaybe)
}

// ----- versioned maps

/// A variable or function name, as the maps key it: copying a changed map
/// node clones every key in it, and a shared name makes that a reference
/// count instead of an allocation.
pub type Name = Rc<str>;

/// The storage of a `VersionedMap`: a persistent map, as Haskell's
/// `Data.Map` is, so a copy shares structure with the map it came from.
/// The analysis runs on one thread, so the nodes are counted with `Rc`.
pub type Storage<V> = GenericOrdMap<Name, Rc<V>, RcK>;

/// `VersionedMap`. The analysis patches a full state per node and
/// invocation, and with deep copies a large script needs gigabytes. The
/// values are shared too: a changed map node copies its entries, and a
/// value is a nest of sets.
#[derive(Clone, Debug)]
pub struct VersionedMap<V: Clone> {
    pub version: i64,
    pub storage: Storage<V>,
}

impl<V: Clone + PartialEq> PartialEq for VersionedMap<V> {
    fn eq(&self, other: &Self) -> bool {
        vm_is_quick_equal(self, other) || self.storage == other.storage
    }
}

fn vm_empty<V: Clone>() -> VersionedMap<V> {
    VersionedMap {
        version: 0,
        storage: Storage::new(),
    }
}

const fn vm_is_quick_equal<V: Clone>(a: &VersionedMap<V>, b: &VersionedMap<V>) -> bool {
    a.version >= 0 && b.version >= 0 && a.version == b.version
}

fn vm_lookup<'m, V: Clone>(k: &str, m: &'m VersionedMap<V>) -> Option<&'m V> {
    m.storage.get(k).map(Rc::as_ref)
}

fn vm_insert<V: Clone>(k: &str, v: V, m: &VersionedMap<V>) -> VersionedMap<V> {
    VersionedMap {
        version: -1,
        storage: m.storage.update(Name::from(k), Rc::new(v)),
    }
}

fn vm_patch<V: Clone + PartialEq>(
    base: &VersionedMap<V>,
    diff: &VersionedMap<V>,
) -> VersionedMap<V> {
    if base.version == 0 {
        return diff.clone();
    }
    if diff.version == 0 {
        return base.clone();
    }
    if vm_is_quick_equal(base, diff) {
        return diff.clone();
    }
    // `M.union diff base`: diff's entries win. The result is built on the
    // base, which the analysis usually keeps, so the two share nodes; only
    // a base much smaller than the diff (a dependency state patched with a
    // full one) is added to the diff instead. A much smaller map is added
    // entry by entry; between maps of similar size `diff` walks the two,
    // skipping the subtrees they share. An entry the result already holds
    // is not inserted again, so no node is copied for it.
    let (b, d) = (base.storage.len(), diff.storage.len());
    if d <= b / 4 {
        let mut out = base.storage.clone();
        for (k, v) in &diff.storage {
            if out.get(k).is_none_or(|old| !Rc::ptr_eq(old, v)) {
                out.insert(Name::clone(k), Rc::clone(v));
            }
        }
        if out.ptr_eq(&base.storage) {
            return base.clone();
        }
        return VersionedMap {
            version: -1,
            storage: out,
        };
    }
    if b <= d / 4 {
        let mut out = diff.storage.clone();
        for (k, v) in &base.storage {
            if !out.contains_key(k) {
                out.insert(Name::clone(k), Rc::clone(v));
            }
        }
        if out.ptr_eq(&diff.storage) {
            return diff.clone();
        }
        return VersionedMap {
            version: -1,
            storage: out,
        };
    }
    let mut out = base.storage.clone();
    for item in base.storage.diff(&diff.storage) {
        match item {
            DiffItem::Add(k, v) | DiffItem::Update { new: (k, v), .. } => {
                out.insert(Name::clone(k), Rc::clone(v));
            }
            DiffItem::Remove(..) => {}
        }
    }
    if out.ptr_eq(&base.storage) {
        return base.clone();
    }
    VersionedMap {
        version: -1,
        storage: out,
    }
}

// ----- internal states

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum FunctionDefinition {
    FunctionUnknown,
    FunctionDefinition(Name, Node, Node),
}

/// `Set FunctionDefinition`, as a sorted vector: a set holds one or two
/// definitions, which a `BTreeSet` stores in a node allocated for eleven.
/// The derived order is the `Data.Set`'s, by members in order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FunctionValue(Vec<FunctionDefinition>);

impl FunctionValue {
    fn single(d: FunctionDefinition) -> Self {
        Self(vec![d])
    }

    fn iter(&self) -> impl Iterator<Item = &FunctionDefinition> {
        self.0.iter()
    }

    fn union(&self, other: &Self) -> Self {
        let mut all = Vec::with_capacity(self.0.len() + other.0.len());
        all.extend(self.0.iter().cloned());
        all.extend(other.0.iter().cloned());
        all.sort();
        all.dedup();
        Self(all)
    }
}

#[derive(Clone, Debug)]
struct InternalState {
    version: i64,
    global_values: VersionedMap<VariableState>,
    local_values: VersionedMap<VariableState>,
    prefix_values: VersionedMap<VariableState>,
    function_targets: VersionedMap<FunctionValue>,
    exit_codes: Option<Rc<BTreeSet<Id>>>,
    is_reachable: Option<bool>,
}

impl PartialEq for InternalState {
    fn eq(&self, other: &Self) -> bool {
        state_is_quick_equal(self, other)
            || (self.global_values == other.global_values
                && self.local_values == other.local_values
                && self.prefix_values == other.prefix_values
                && self.function_targets == other.function_targets
                && self.is_reachable == other.is_reachable)
    }
}

fn new_internal_state() -> InternalState {
    InternalState {
        version: 0,
        global_values: vm_empty(),
        local_values: vm_empty(),
        prefix_values: vm_empty(),
        function_targets: vm_empty(),
        exit_codes: None,
        is_reachable: None,
    }
}

const fn modified(mut s: InternalState) -> InternalState {
    s.version = -1;
    s
}

fn unreachable_state() -> InternalState {
    let mut s = new_internal_state();
    s.is_reachable = Some(false);
    modified(s)
}

const fn default_properties() -> VariableProperties {
    VariableProperties::single(PropSet::from_bits(0))
}

const fn unknown_variable_value() -> VariableValue {
    VariableValue {
        literal_value: None,
        space_status: SpaceStatus::SpaceStatusDirty,
        numerical_status: NumericalStatus::NumericalStatusUnknown,
    }
}

const fn empty_variable_value() -> VariableValue {
    VariableValue {
        literal_value: Some(String::new()),
        space_status: SpaceStatus::SpaceStatusEmpty,
        numerical_status: NumericalStatus::NumericalStatusEmpty,
    }
}

const fn unknown_integer_value() -> VariableValue {
    VariableValue {
        literal_value: None,
        space_status: SpaceStatus::SpaceStatusClean,
        numerical_status: NumericalStatus::NumericalStatusDefinitely,
    }
}

const fn unknown_variable_state() -> VariableState {
    VariableState {
        variable_value: unknown_variable_value(),
        variable_properties: default_properties(),
    }
}

const fn unset_variable_state() -> VariableState {
    VariableState {
        variable_value: empty_variable_value(),
        variable_properties: default_properties(),
    }
}

fn unknown_function_value() -> FunctionValue {
    FunctionValue::single(FunctionDefinition::FunctionUnknown)
}

fn insert_global(name: &str, value: VariableState, state: InternalState) -> InternalState {
    let mut s = state;
    s.global_values = vm_insert(name, value, &s.global_values);
    modified(s)
}

fn insert_local(name: &str, value: VariableState, state: InternalState) -> InternalState {
    let mut s = state;
    s.local_values = vm_insert(name, value, &s.local_values);
    modified(s)
}

fn insert_prefix(name: &str, value: VariableState, state: InternalState) -> InternalState {
    let mut s = state;
    s.prefix_values = vm_insert(name, value, &s.prefix_values);
    modified(s)
}

fn insert_function(name: &str, value: FunctionValue, state: InternalState) -> InternalState {
    let mut s = state;
    s.function_targets = vm_insert(name, value, &s.function_targets);
    modified(s)
}

fn add_properties(props: PropSet, state: VariableState) -> VariableState {
    VariableState {
        variable_value: state.variable_value,
        variable_properties: state.variable_properties.map(|s| s.union(props)),
    }
}

fn remove_properties(props: PropSet, state: VariableState) -> VariableState {
    VariableState {
        variable_value: state.variable_value,
        variable_properties: state.variable_properties.map(|s| s.difference(props)),
    }
}

fn set_exit_codes(set: BTreeSet<Id>, state: InternalState) -> InternalState {
    let mut s = state;
    s.exit_codes = Some(Rc::new(set));
    modified(s)
}

fn create_environment_state() -> InternalState {
    let spaceless = VariableState {
        variable_value: VariableValue {
            literal_value: None,
            space_status: SpaceStatus::SpaceStatusClean,
            numerical_status: NumericalStatus::NumericalStatusUnknown,
        },
        variable_properties: default_properties(),
    };
    let integer = VariableState {
        variable_value: unknown_integer_value(),
        variable_properties: default_properties(),
    };
    let mut s = new_internal_state();
    for name in data::INTERNAL_VARIABLES {
        s = insert_global(name, unknown_variable_state(), s);
    }
    for name in data::VARIABLES_WITHOUT_SPACES {
        s = insert_global(name, spaceless.clone(), s);
    }
    for name in data::SPECIAL_INTEGER_VARIABLES {
        s = insert_global(name, integer.clone(), s);
    }
    s
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(
    clippy::enum_variant_names,
    reason = "the variant names are ShellCheck's StateDependency constructors"
)]
enum StateDependency {
    DepState(Scope, String, VariableState),
    DepProperties(Scope, String, VariableProperties),
    DepFunction(String, FunctionValue),
    DepIsRecursive(Node, bool),
    DepExitCodes(BTreeSet<Id>),
}

fn deps_to_state(set: &BTreeSet<StateDependency>) -> InternalState {
    let mut state = new_internal_state();
    for dep in set {
        state = match dep {
            StateDependency::DepFunction(name, val) => insert_function(name, val.clone(), state),
            StateDependency::DepState(scope, name, val) => {
                insert_in(true, *scope, name, val.clone(), state)
            }
            StateDependency::DepProperties(scope, name, props) => {
                let v = VariableState {
                    variable_value: unknown_variable_value(),
                    variable_properties: *props,
                };
                insert_in(false, *scope, name, v, state)
            }
            StateDependency::DepIsRecursive(..) => state,
            StateDependency::DepExitCodes(s) => set_exit_codes(s.clone(), state),
        };
    }
    state
}

fn insert_in(
    overwrite: bool,
    scope: Scope,
    name: &str,
    val: VariableState,
    state: InternalState,
) -> InternalState {
    let already_exists = match scope {
        Scope::PrefixScope => vm_lookup(name, &state.prefix_values).is_some(),
        Scope::LocalScope => vm_lookup(name, &state.local_values).is_some(),
        Scope::GlobalScope => vm_lookup(name, &state.global_values).is_some(),
    };
    if overwrite || !already_exists {
        match scope {
            Scope::PrefixScope => insert_prefix(name, val, state),
            Scope::LocalScope => insert_local(name, val, state),
            Scope::GlobalScope => insert_global(name, val, state),
        }
    } else {
        state
    }
}

fn merge_variable_state(a: &VariableState, b: &VariableState) -> VariableState {
    VariableState {
        variable_value: merge_variable_value(&a.variable_value, &b.variable_value),
        variable_properties: a.variable_properties.union(b.variable_properties),
    }
}

fn merge_variable_value(a: &VariableValue, b: &VariableValue) -> VariableValue {
    VariableValue {
        literal_value: if a.literal_value == b.literal_value {
            a.literal_value.clone()
        } else {
            None
        },
        space_status: merge_space_status(a.space_status, b.space_status),
        numerical_status: merge_numerical_status(a.numerical_status, b.numerical_status),
    }
}

const fn merge_space_status(a: SpaceStatus, b: SpaceStatus) -> SpaceStatus {
    match (a, b) {
        (SpaceStatus::SpaceStatusEmpty, y) => y,
        (x, SpaceStatus::SpaceStatusEmpty) => x,
        (SpaceStatus::SpaceStatusClean, SpaceStatus::SpaceStatusClean) => {
            SpaceStatus::SpaceStatusClean
        }
        _ => SpaceStatus::SpaceStatusDirty,
    }
}

const fn merge_numerical_status(a: NumericalStatus, b: NumericalStatus) -> NumericalStatus {
    use NumericalStatus::{
        NumericalStatusDefinitely, NumericalStatusEmpty, NumericalStatusMaybe,
        NumericalStatusUnknown,
    };
    match (a, b) {
        (NumericalStatusDefinitely, NumericalStatusDefinitely) => NumericalStatusDefinitely,
        (NumericalStatusDefinitely, _) | (_, NumericalStatusDefinitely) => NumericalStatusMaybe,
        (NumericalStatusMaybe, _) | (_, NumericalStatusMaybe) => NumericalStatusMaybe,
        (NumericalStatusEmpty, NumericalStatusEmpty) => NumericalStatusEmpty,
        _ => NumericalStatusUnknown,
    }
}

const fn state_is_quick_equal(a: &InternalState, b: &InternalState) -> bool {
    a.version >= 0 && b.version >= 0 && a.version == b.version
}

/// `patchState`: overwrite a base state with a diff.
fn patch_state(base: &InternalState, diff: &InternalState) -> InternalState {
    if diff.version == 0 {
        return base.clone();
    }
    if base.version == 0 {
        return diff.clone();
    }
    if state_is_quick_equal(base, diff) {
        return diff.clone();
    }
    InternalState {
        version: -1,
        global_values: vm_patch(&base.global_values, &diff.global_values),
        local_values: vm_patch(&base.local_values, &diff.local_values),
        prefix_values: vm_patch(&base.prefix_values, &diff.prefix_values),
        function_targets: vm_patch(&base.function_targets, &diff.function_targets),
        exit_codes: diff.exit_codes.clone().or_else(|| base.exit_codes.clone()),
        is_reachable: diff.is_reachable.or(base.is_reachable),
    }
}

// ----- the context

struct StackEntry {
    entry_point: Node,
    is_function_call: bool,
    call_site: Node,
    dependencies: BTreeSet<StateDependency>,
    stack_state: InternalState,
}

struct Frame {
    node: Node,
    input: InternalState,
    output: InternalState,
}

type StateMap = BTreeMap<Node, (InternalState, InternalState)>;

struct Ctx<'g> {
    graph: &'g Graph,
    counter: i64,
    cache: IntMap<Node, Vec<(BTreeSet<StateDependency>, InternalState)>>,
    enable_cache: bool,
    invocations: BTreeMap<Vec<Node>, (BTreeSet<StateDependency>, StateMap)>,
    /// Innermost last.
    stack: Vec<StackEntry>,
    /// The current frame is the last.
    frames: Vec<Frame>,
    contexts: IntMap<Node, NodeContext>,
}

/// A node's label and adjacency, as `context graph node` gives them.
struct NodeContext {
    label: CfNode,
    incoming_flow: Vec<Node>,
    outgoing: Vec<Node>,
}

impl Ctx<'_> {
    fn frame(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("a frame")
    }

    fn input(&self) -> &InternalState {
        &self.frames.last().expect("a frame").input
    }

    fn output(&self) -> &InternalState {
        &self.frames.last().expect("a frame").output
    }

    fn set_output(&mut self, s: InternalState) {
        self.frame().output = s;
    }

    fn modify_output(&mut self, f: impl FnOnce(InternalState) -> InternalState) {
        let out = std::mem::replace(&mut self.frame().output, new_internal_state());
        self.frame().output = f(out);
    }

    fn patch_output(&mut self, diff: &InternalState) {
        let new = patch_state(self.output(), diff);
        self.set_output(new);
    }

    const fn next_version(&mut self) -> i64 {
        let n = self.counter;
        self.counter += 1;
        n
    }

    fn version_map<V: Clone>(&mut self, m: VersionedMap<V>) -> VersionedMap<V> {
        if m.version >= 0 {
            m
        } else {
            let v = self.next_version();
            VersionedMap {
                version: v,
                storage: m.storage,
            }
        }
    }

    fn version_state(&mut self, state: InternalState) -> InternalState {
        if state.version >= 0 {
            return state;
        }
        let me = self.next_version();
        let globals = self.version_map(state.global_values);
        let locals = self.version_map(state.local_values);
        let functions = self.version_map(state.function_targets);
        InternalState {
            version: me,
            global_values: globals,
            local_values: locals,
            prefix_values: state.prefix_values,
            function_targets: functions,
            exit_codes: state.exit_codes,
            is_reachable: state.is_reachable,
        }
    }

    // ----- lookups

    /// `lookupStack'`.
    fn lookup_stack<V: Clone>(
        &mut self,
        function_only: bool,
        get: &dyn Fn(&InternalState) -> Option<V>,
        dep: &dyn Fn(&V) -> StateDependency,
        def: V,
    ) -> V {
        if let Some(v) = get(self.input()) {
            return v;
        }
        self.lookup_stack_from(self.stack.len(), function_only, get, dep, def)
    }

    fn lookup_stack_from<V: Clone>(
        &mut self,
        level: usize,
        function_only: bool,
        get: &dyn Fn(&InternalState) -> Option<V>,
        dep: &dyn Fn(&V) -> StateDependency,
        def: V,
    ) -> V {
        if level == 0 {
            return def;
        }
        let i = level - 1;
        if function_only && self.stack[i].is_function_call {
            return def;
        }
        let res = match get(&self.stack[i].stack_state) {
            Some(v) => v,
            None => self.lookup_stack_from(i, function_only, get, dep, def),
        };
        let d = dep(&res);
        self.stack[i].dependencies.insert(d);
        res
    }

    /// `peekStack`: like `lookupStack` without dependencies.
    fn peek_stack<V: Clone>(&self, get: &dyn Fn(&InternalState) -> Option<V>, def: V) -> V {
        if let Some(v) = get(self.input()) {
            return v;
        }
        for s in self.stack.iter().rev() {
            if let Some(v) = get(&s.stack_state) {
                return v;
            }
        }
        def
    }

    fn read_variable_with_scope(&mut self, name: &str) -> (VariableState, Scope) {
        let n = name.to_string();
        self.lookup_stack(
            false,
            &|s| get_variable_with_scope(s, &n),
            &|(val, scope): &(VariableState, Scope)| {
                StateDependency::DepState(*scope, n.clone(), val.clone())
            },
            (unknown_variable_state(), Scope::GlobalScope),
        )
    }

    fn read_variable_properties_with_scope(&mut self, name: &str) -> (VariableProperties, Scope) {
        let n = name.to_string();
        self.lookup_stack(
            false,
            &|s| get_variable_with_scope(s, &n).map(|(v, scope)| (v.variable_properties, scope)),
            &|(val, scope): &(VariableProperties, Scope)| {
                StateDependency::DepProperties(*scope, n.clone(), *val)
            },
            (default_properties(), Scope::GlobalScope),
        )
    }

    fn read_variable_scope(&mut self, name: &str) -> Scope {
        self.read_variable_properties_with_scope(name).1
    }

    fn read_variable(&mut self, name: &str) -> VariableState {
        self.read_variable_with_scope(name).0
    }

    fn read_global(&mut self, name: &str) -> VariableState {
        let n = name.to_string();
        self.lookup_stack(
            false,
            &|s| vm_lookup(&n, &s.global_values).cloned(),
            &|v: &VariableState| {
                StateDependency::DepState(Scope::GlobalScope, n.clone(), v.clone())
            },
            unknown_variable_state(),
        )
    }

    fn read_global_properties(&mut self, name: &str) -> VariableProperties {
        let n = name.to_string();
        self.lookup_stack(
            false,
            &|s| vm_lookup(&n, &s.global_values).map(|v| v.variable_properties),
            &|v: &VariableProperties| {
                StateDependency::DepProperties(Scope::GlobalScope, n.clone(), *v)
            },
            default_properties(),
        )
    }

    fn read_local(&mut self, name: &str) -> VariableState {
        let n = name.to_string();
        self.lookup_stack(
            true,
            &|s| vm_lookup(&n, &s.local_values).cloned(),
            &|v: &VariableState| StateDependency::DepState(Scope::LocalScope, n.clone(), v.clone()),
            unset_variable_state(),
        )
    }

    fn read_local_properties(&mut self, name: &str) -> VariableProperties {
        let n = name.to_string();
        self.lookup_stack(
            true,
            &|s| {
                vm_lookup(&n, &s.local_values)
                    .map(|v| (v.variable_properties, Scope::LocalScope))
                    .or_else(|| {
                        vm_lookup(&n, &s.prefix_values)
                            .map(|v| (v.variable_properties, Scope::PrefixScope))
                    })
            },
            &|(val, scope): &(VariableProperties, Scope)| {
                StateDependency::DepProperties(*scope, n.clone(), *val)
            },
            (default_properties(), Scope::LocalScope),
        )
        .0
    }

    fn read_function(&mut self, name: &str) -> FunctionValue {
        let n = name.to_string();
        self.lookup_stack(
            false,
            &|s| vm_lookup(&n, &s.function_targets).cloned(),
            &|v: &FunctionValue| StateDependency::DepFunction(n.clone(), v.clone()),
            unknown_function_value(),
        )
    }

    fn read_exit_codes(&mut self) -> BTreeSet<Id> {
        self.lookup_stack(
            false,
            &|s| s.exit_codes.as_ref().map(|r| (**r).clone()),
            &|v: &BTreeSet<Id>| StateDependency::DepExitCodes(v.clone()),
            BTreeSet::new(),
        )
    }

    // ----- writes

    fn write_variable(&mut self, name: &str, val: VariableState) {
        match self.read_variable_scope(name) {
            Scope::GlobalScope => self.write_global(name, val),
            Scope::LocalScope | Scope::PrefixScope => self.write_local(name, val),
        }
    }

    fn write_global(&mut self, name: &str, val: VariableState) {
        self.modify_output(|s| insert_global(name, val, s));
    }

    fn write_local(&mut self, name: &str, val: VariableState) {
        self.modify_output(|s| insert_local(name, val, s));
    }

    fn write_prefix(&mut self, name: &str, val: VariableState) {
        self.modify_output(|s| insert_prefix(name, val, s));
    }

    fn write_function(&mut self, name: &str, val: FunctionDefinition) {
        self.modify_output(|s| insert_function(name, FunctionValue::single(val), s));
    }

    fn update_variable_value(&mut self, name: &str, val: VariableValue) {
        let (props, scope) = self.read_variable_properties_with_scope(name);
        let v = VariableState {
            variable_value: val,
            variable_properties: props,
        };
        match scope {
            Scope::GlobalScope => self.write_global(name, v),
            Scope::LocalScope | Scope::PrefixScope => self.write_local(name, v),
        }
    }

    fn update_global_value(&mut self, name: &str, val: VariableValue) {
        let props = self.read_global_properties(name);
        self.write_global(
            name,
            VariableState {
                variable_value: val,
                variable_properties: props,
            },
        );
    }

    fn update_local_value(&mut self, name: &str, val: VariableValue) {
        let props = self.read_local_properties(name);
        self.write_local(
            name,
            VariableState {
                variable_value: val,
                variable_properties: props,
            },
        );
    }

    fn update_prefix_value(&mut self, name: &str, val: VariableValue) {
        self.write_prefix(
            name,
            VariableState {
                variable_value: val,
                variable_properties: default_properties(),
            },
        );
    }

    // ----- merges

    /// `mergeState`.
    fn merge_state(&mut self, a: &InternalState, b: &InternalState) -> InternalState {
        let old = std::mem::replace(&mut self.frame().input, new_internal_state());
        let x = self.merge(a, b);
        self.frame().input = old;
        x
    }

    fn merge(&mut self, a: &InternalState, b: &InternalState) -> InternalState {
        let (ra, rb) = (a.is_reachable, b.is_reachable);
        assert!(
            !((ra == Some(true) && rb == Some(false)) || (ra == Some(false) && rb == Some(true))),
            "ShellCheck internal error, please report: Unexpected merge of reachable and unreachable state"
        );
        if ra == Some(false) && rb == Some(false) {
            return unreachable_state();
        }
        if a.version >= 0 && b.version >= 0 && a.version == b.version {
            return a.clone();
        }
        let globals = self.merge_maps(
            &merge_variable_state,
            &|c, k| c.read_global(k),
            &a.global_values,
            &b.global_values,
        );
        let locals = self.merge_maps(
            &merge_variable_state,
            &|c, k| c.read_variable(k),
            &a.local_values,
            &b.local_values,
        );
        let prefix = self.merge_maps(
            &merge_variable_state,
            &|c, k| c.read_variable(k),
            &a.prefix_values,
            &b.prefix_values,
        );
        let funcs = self.merge_maps(
            &FunctionValue::union,
            &|c, k| c.read_function(k),
            &a.function_targets,
            &b.function_targets,
        );
        let exit_codes = match (&a.exit_codes, &b.exit_codes) {
            (None, None) => None,
            (Some(v), None) | (None, Some(v)) => {
                let other = self.read_exit_codes();
                Some(Rc::new(v.union(&other).copied().collect()))
            }
            (Some(v1), Some(v2)) => Some(Rc::new(v1.union(v2).copied().collect())),
        };
        let is_reachable = match (ra, rb) {
            (Some(x), Some(y)) => Some(x && y),
            _ => None,
        };
        InternalState {
            version: -1,
            global_values: globals,
            local_values: locals,
            prefix_values: prefix,
            function_targets: funcs,
            exit_codes,
            is_reachable,
        }
    }

    /// `mergeMaps`. A key in both maps with equal values keeps its value,
    /// as every merger is idempotent, so the result starts as a copy of `a`
    /// and only the entries that differ are visited: the maps are usually
    /// versions of one another, and `diff` skips the subtrees they share.
    /// A key in one map only is merged with the value the stack gives it,
    /// which `reader` records as a dependency.
    fn merge_maps<V: Clone + PartialEq>(
        &mut self,
        merger: &dyn Fn(&V, &V) -> V,
        reader: &dyn Fn(&mut Self, &str) -> V,
        a: &VersionedMap<V>,
        b: &VersionedMap<V>,
    ) -> VersionedMap<V> {
        if vm_is_quick_equal(a, b) {
            return a.clone();
        }
        let a_last = a.storage.get_max().map(|(k, _)| Name::clone(k));
        let mut out = a.storage.clone();
        // A merge that leaves a's value as it was leaves `out` untouched:
        // no node of it is copied.
        let merge_into_a = |out: &mut Storage<V>, k: &Name, va: &V, merged: V| {
            if merged != *va {
                out.insert(Name::clone(k), Rc::new(merged));
            }
        };
        for item in a.storage.diff(&b.storage) {
            match item {
                DiffItem::Remove(k, va) => {
                    let other = reader(self, k);
                    merge_into_a(&mut out, k, va, merger(va, &other));
                }
                DiffItem::Add(k, vb) => {
                    let other = reader(self, k);
                    // `f l [] b = f l b []`: once `a` is exhausted, b's
                    // entries are merged as the first argument.
                    let value = if a_last.as_ref().is_none_or(|last| k > last) {
                        merger(vb, &other)
                    } else {
                        merger(&other, vb)
                    };
                    out.insert(Name::clone(k), Rc::new(value));
                }
                DiffItem::Update {
                    old: (k, va),
                    new: (_, vb),
                } => {
                    merge_into_a(&mut out, k, va, merger(va, vb));
                }
            }
        }
        // Nothing changed: the result is `a`, and keeping its version keeps
        // later comparisons with it quick.
        if out.ptr_eq(&a.storage) {
            return a.clone();
        }
        VersionedMap {
            version: -1,
            storage: out,
        }
    }

    fn merge_states(&mut self, def: InternalState, list: &[InternalState]) -> InternalState {
        match list.split_first() {
            None => def,
            Some((first, rest)) => {
                let mut acc = first.clone();
                for s in rest {
                    acc = self.merge_state(&acc, s);
                }
                acc
            }
        }
    }

    // ----- dependencies and caching

    fn fulfills_dependency(&self, entry: Node, dep: &StateDependency) -> bool {
        let peek = |scope: Scope, name: &str| -> (VariableState, Scope) {
            let def = if scope == Scope::GlobalScope {
                (unknown_variable_state(), Scope::GlobalScope)
            } else {
                (unset_variable_state(), Scope::LocalScope)
            };
            self.peek_stack(&|s| get_variable_with_scope(s, name), def)
        };
        match dep {
            StateDependency::DepState(scope, name, val) => {
                peek(*scope, name) == (val.clone(), *scope)
            }
            StateDependency::DepProperties(scope, name, props) => {
                let (state, s) = peek(*scope, name);
                *scope == s && state.variable_properties == *props
            }
            StateDependency::DepFunction(name, val) => {
                self.peek_stack(
                    &|s| vm_lookup(name, &s.function_targets).cloned(),
                    unknown_function_value(),
                ) == *val
            }
            StateDependency::DepIsRecursive(node, val) => {
                if *node == entry {
                    true
                } else {
                    *val == self.stack.iter().any(|f| f.entry_point == *node)
                }
            }
            StateDependency::DepExitCodes(val) => {
                self.peek_stack(
                    &|s| s.exit_codes.as_ref().map(|r| (**r).clone()),
                    BTreeSet::new(),
                ) == *val
            }
        }
    }

    fn fulfills_dependencies(&self, entry: Node, deps: &BTreeSet<StateDependency>) -> bool {
        deps.iter().all(|d| self.fulfills_dependency(entry, d))
    }

    fn get_cache(&self, node: Node) -> Option<InternalState> {
        if !self.enable_cache {
            return None;
        }
        let entries = self.cache.get(&node)?;
        entries
            .iter()
            .find(|(deps, _)| self.fulfills_dependencies(node, deps))
            .map(|(_, v)| v.clone())
    }

    /// `runCached`.
    fn run_cached(
        &mut self,
        node: Node,
        f: &mut dyn FnMut(&mut Self) -> (BTreeSet<StateDependency>, InternalState),
    ) {
        if let Some(v) = self.get_cache(node) {
            self.patch_output(&v);
        } else {
            let (deps, diff) = f(self);
            let entry = self.cache.entry(node).or_default();
            if entry.is_empty() {
                entry.push((deps, diff.clone()));
            } else {
                entry.truncate(CACHE_ENTRIES);
                entry.insert(0, (deps, diff.clone()));
            }
            self.patch_output(&diff);
        }
    }

    /// `withNewStackFrame`.
    fn with_new_stack_frame<T>(
        &mut self,
        node: Node,
        is_call: bool,
        f: &mut dyn FnMut(&mut Self) -> T,
    ) -> (T, StackEntry) {
        let entry = StackEntry {
            entry_point: node,
            is_function_call: is_call,
            call_site: self.frames.last().expect("a frame").node,
            dependencies: BTreeSet::new(),
            stack_state: self.output().clone(),
        };
        self.stack.push(entry);
        self.frames.push(Frame {
            node,
            input: new_internal_state(),
            output: new_internal_state(),
        });
        let x = f(self);
        self.frames.pop();
        let entry = self.stack.pop().expect("the pushed entry");
        (x, entry)
    }

    /// `wouldBeRecursive`.
    fn would_be_recursive(&mut self, node: Node) -> bool {
        let found = self.stack.iter().rposition(|s| s.entry_point == node);
        let (from, res) = match found {
            // Entries from the innermost down to the match are visited.
            Some(i) => (i, true),
            None => (0, false),
        };
        for s in &mut self.stack[from..] {
            s.dependencies
                .insert(StateDependency::DepIsRecursive(node, res));
        }
        res
    }

    /// `registerFlowResult`.
    fn register_flow_result(
        &mut self,
        entry: Node,
        states: StateMap,
        deps: BTreeSet<StateDependency>,
    ) {
        let current = self.frames.last().expect("a frame").node;
        let mut path = vec![entry, current];
        path.extend(self.stack.iter().rev().map(|s| s.call_site));
        self.invocations.insert(path, (deps, states));
    }

    // ----- transfer

    fn transfer(&mut self, label: &CfNode) {
        match label {
            CfNode::CFStructuralNode
            | CfNode::CFEntryPoint(_)
            | CfNode::CFImpliedExit
            | CfNode::CFResolvedExit
            | CfNode::CFSetBackgroundPid(_) => {}
            CfNode::CFExecuteCommand(cmd) => self.transfer_command(cmd.as_deref()),
            CfNode::CFExecuteSubshell(_, entry, exit) => self.transfer_subshell(*entry, *exit),
            CfNode::CFApplyEffects(effects) => {
                for IdTagged(_, f) in effects {
                    self.transfer_effect(f);
                }
            }
            CfNode::CFSetExitCode(id) => {
                let mut set = BTreeSet::new();
                set.insert(*id);
                self.modify_output(|s| set_exit_codes(set, s));
            }
            CfNode::CFUnresolvedExit | CfNode::CFUnreachable => {
                self.patch_output(&unreachable_state());
            }
            CfNode::CFDropPrefixAssignments => self.modify_output(|mut c| {
                c.prefix_values = vm_empty();
                modified(c)
            }),
        }
    }

    fn transfer_subshell(&mut self, entry: Node, exit: Node) {
        let initial = self.output().clone();
        self.run_cached(entry, &mut |ctx| {
            let (states, frame) =
                ctx.with_new_stack_frame(entry, false, &mut |c| c.dataflow(entry));
            let (_, res) = states
                .get(&exit)
                .cloned()
                .expect("ShellCheck internal error, please report: Subshell has no exit");
            let deps = frame.dependencies;
            ctx.register_flow_result(entry, states, deps.clone());
            (deps, res)
        });
        let res_exit = self.output().exit_codes.clone();
        let mut restored = initial;
        restored.exit_codes = res_exit;
        self.set_output(restored);
    }

    fn transfer_command(&mut self, cmd: Option<&str>) {
        let Some(name) = cmd else {
            return;
        };
        let targets = self.read_function(name);
        let original = self.output().clone();
        let mut branches = Vec::new();
        for t in targets.iter() {
            self.set_output(original.clone());
            self.transfer_function_value(t);
            branches.push(self.output().clone());
        }
        let merged = self.merge_states(original.clone(), &branches);
        let patched = patch_state(&original, &merged);
        self.set_output(patched);
    }

    fn transfer_function_value(&mut self, func: &FunctionDefinition) {
        let FunctionDefinition::FunctionDefinition(_, entry, exit) = func else {
            return;
        };
        let (entry, exit) = (*entry, *exit);
        if self.would_be_recursive(entry) {
            return;
        }
        self.run_cached(entry, &mut |ctx| {
            let (states, frame) = ctx.with_new_stack_frame(entry, true, &mut |c| c.dataflow(entry));
            let deps = frame.dependencies;
            let res = match states.get(&exit) {
                Some((_, output)) => {
                    let mut o = output.clone();
                    o.local_values = vm_empty();
                    modified(o)
                }
                None => unreachable_state(),
            };
            ctx.register_flow_result(entry, states, deps.clone());
            (deps, res)
        });
    }

    fn transfer_effect(&mut self, effect: &CfEffect) {
        match effect {
            CfEffect::CFReadVariable(name) => {
                if name == "?" {
                    self.read_exit_codes();
                } else {
                    self.read_variable(name);
                }
            }
            CfEffect::CFWriteVariable(name, value) => {
                let v = self.cf_value_to_variable_value(value);
                self.update_variable_value(name, v);
            }
            CfEffect::CFWriteGlobal(name, value) => {
                let v = self.cf_value_to_variable_value(value);
                self.update_global_value(name, v);
            }
            CfEffect::CFWriteLocal(name, value) => {
                let v = self.cf_value_to_variable_value(value);
                self.update_local_value(name, v);
            }
            CfEffect::CFWritePrefix(name, value) => {
                let v = self.cf_value_to_variable_value(value);
                self.update_prefix_value(name, v);
            }
            CfEffect::CFSetProps(scope, name, props) => match scope {
                None => {
                    let state = self.read_variable(name);
                    self.write_variable(name, add_properties(*props, state));
                }
                Some(Scope::GlobalScope) => {
                    let state = self.read_global(name);
                    self.write_global(name, add_properties(*props, state));
                }
                Some(Scope::LocalScope | Scope::PrefixScope) => {
                    let state = self.read_local(name);
                    self.write_local(name, add_properties(*props, state));
                }
            },
            CfEffect::CFUnsetProps(scope, name, props) => match scope {
                None => {
                    let state = self.read_variable(name);
                    self.write_variable(name, remove_properties(*props, state));
                }
                Some(Scope::GlobalScope) => {
                    let state = self.read_global(name);
                    self.write_global(name, remove_properties(*props, state));
                }
                Some(Scope::LocalScope | Scope::PrefixScope) => {
                    let state = self.read_local(name);
                    self.write_local(name, remove_properties(*props, state));
                }
            },
            CfEffect::CFUndefineVariable(name) | CfEffect::CFUndefineNameref(name) => {
                self.write_variable(name, unset_variable_state());
            }
            CfEffect::CFUndefineFunction(name) => {
                self.write_function(name, FunctionDefinition::FunctionUnknown);
            }
            CfEffect::CFUndefine(name) => {
                self.write_variable(name, unset_variable_state());
                self.write_function(name, FunctionDefinition::FunctionUnknown);
            }
            CfEffect::CFDefineFunction(name, _, entry, exit) => {
                self.write_function(
                    name,
                    FunctionDefinition::FunctionDefinition(
                        Name::from(name.as_str()),
                        *entry,
                        *exit,
                    ),
                );
            }
            CfEffect::CFHintArray(_) => {}
        }
    }

    fn cf_value_to_variable_value(&mut self, val: &CfValue) -> VariableValue {
        match val {
            CfValue::CFValueArray | CfValue::CFValueString => unknown_variable_value(),
            CfValue::CFValueInteger => unknown_integer_value(),
            CfValue::CFValueComputed(_, parts) => {
                let mut acc = empty_variable_value();
                for part in parts {
                    let next = self.compute_value(part);
                    acc = append_variable_value(&acc, &next);
                }
                acc
            }
        }
    }

    fn compute_value(&mut self, part: &CfStringPart) -> VariableValue {
        match part {
            CfStringPart::CFStringLiteral(s) => literal_to_variable_value(s),
            CfStringPart::CFStringInteger => unknown_integer_value(),
            CfStringPart::CFStringUnknown => unknown_variable_value(),
            CfStringPart::CFStringVariable(name) => {
                let state = self.read_variable(name);
                if state
                    .variable_properties
                    .all_contain(CfVariableProp::CFVPInteger)
                {
                    unknown_integer_value()
                } else {
                    state.variable_value
                }
            }
        }
    }

    // ----- the flow

    fn node_context(&mut self, node: Node) -> &NodeContext {
        if !self.contexts.contains_key(&node) {
            let (incoming, _, label, outgoing) = self.graph.context(node);
            let nc = NodeContext {
                label,
                incoming_flow: incoming
                    .into_iter()
                    .filter(|(l, _)| *l == CfEdge::CFEFlow)
                    .map(|(_, n)| n)
                    .collect(),
                outgoing: outgoing.into_iter().map(|(_, n)| n).collect(),
            };
            self.contexts.insert(node, nc);
        }
        &self.contexts[&node]
    }

    /// `dataflow`.
    fn dataflow(&mut self, entry: Node) -> StateMap {
        let mut pending: BTreeSet<Node> = BTreeSet::new();
        pending.insert(entry);
        let mut states: StateMap = BTreeMap::new();
        let prev_input = self.input().clone();
        let prev_output = self.output().clone();
        let mut n = ITERATION_COUNT;
        loop {
            assert!(
                n != 0,
                "ShellCheck internal error, please report: DFA did not reach fix point"
            );
            if n == FALLBACK_THRESHOLD {
                self.enable_cache = false;
            }
            let Some(next) = pending.pop_first() else {
                break;
            };
            let nexts = self.process(&mut states, next);
            pending.extend(nexts);
            n -= 1;
        }
        self.frame().input = prev_input;
        self.frame().output = prev_output;
        states
    }

    fn process(&mut self, states: &mut StateMap, node: Node) -> Vec<Node> {
        let (label, incoming, outgoing) = {
            let nc = self.node_context(node);
            (
                nc.label.clone(),
                nc.incoming_flow.clone(),
                nc.outgoing.clone(),
            )
        };
        let inputs: Vec<InternalState> = incoming
            .iter()
            .filter_map(|c| states.get(c).map(|(_, o)| o.clone()))
            .filter(|c| c.is_reachable != Some(false))
            .collect();
        let input = if incoming.is_empty() {
            new_internal_state()
        } else {
            match inputs.split_first() {
                None => unreachable_state(),
                Some((x, rest)) => {
                    let mut acc = x.clone();
                    for r in rest {
                        acc = self.merge_state(&acc, r);
                    }
                    acc
                }
            }
        };
        {
            let f = self.frame();
            f.input = input.clone();
            f.output = input.clone();
            f.node = node;
        }
        self.transfer(&label);
        let new_output = self.output().clone();
        let result = if outgoing.len() >= 2 {
            self.version_state(new_output)
        } else {
            new_output
        };
        let old = states.insert(node, (input, result.clone()));
        match old {
            None => outgoing,
            Some((_, old_output)) => {
                if old_output == result {
                    Vec::new()
                } else {
                    outgoing
                }
            }
        }
    }
}

fn get_variable_with_scope(s: &InternalState, name: &str) -> Option<(VariableState, Scope)> {
    let n = name.to_string();
    if let Some(v) = vm_lookup(&n, &s.prefix_values) {
        return Some((v.clone(), Scope::PrefixScope));
    }
    if let Some(v) = vm_lookup(&n, &s.local_values) {
        return Some((v.clone(), Scope::LocalScope));
    }
    if let Some(v) = vm_lookup(&n, &s.global_values) {
        return Some((v.clone(), Scope::GlobalScope));
    }
    None
}

fn append_variable_value(a: &VariableValue, b: &VariableValue) -> VariableValue {
    VariableValue {
        literal_value: match (&a.literal_value, &b.literal_value) {
            (Some(x), Some(y)) => Some(format!("{x}{y}")),
            _ => None,
        },
        space_status: append_space_status(a.space_status, b.space_status),
        numerical_status: append_numerical_status(a.numerical_status, b.numerical_status),
    }
}

const fn append_space_status(a: SpaceStatus, b: SpaceStatus) -> SpaceStatus {
    match (a, b) {
        (SpaceStatus::SpaceStatusEmpty, _) => b,
        (_, SpaceStatus::SpaceStatusEmpty) => a,
        (SpaceStatus::SpaceStatusClean, SpaceStatus::SpaceStatusClean) => a,
        _ => SpaceStatus::SpaceStatusDirty,
    }
}

const fn append_numerical_status(a: NumericalStatus, b: NumericalStatus) -> NumericalStatus {
    use NumericalStatus::{
        NumericalStatusDefinitely, NumericalStatusEmpty, NumericalStatusMaybe,
        NumericalStatusUnknown,
    };
    match (a, b) {
        (NumericalStatusEmpty, x) | (x, NumericalStatusEmpty) => x,
        (NumericalStatusDefinitely, NumericalStatusDefinitely) => NumericalStatusDefinitely,
        (NumericalStatusUnknown, _) | (_, NumericalStatusUnknown) => NumericalStatusUnknown,
        _ => NumericalStatusMaybe,
    }
}

fn literal_to_variable_value(s: &str) -> VariableValue {
    VariableValue {
        literal_value: Some(s.to_string()),
        space_status: literal_to_space_status(s),
        numerical_status: literal_to_numerical_status(s),
    }
}

fn literal_to_space_status(s: &str) -> SpaceStatus {
    if s.is_empty() {
        SpaceStatus::SpaceStatusEmpty
    } else if s.chars().all(|c| !" \t\n*?[".contains(c)) {
        SpaceStatus::SpaceStatusClean
    } else {
        SpaceStatus::SpaceStatusDirty
    }
}

fn literal_to_numerical_status(s: &str) -> NumericalStatus {
    if s.is_empty() {
        return NumericalStatus::NumericalStatusEmpty;
    }
    let rest = s.strip_prefix('-').unwrap_or(s);
    if rest.chars().all(|c| c.is_ascii_digit()) {
        NumericalStatus::NumericalStatusDefinitely
    } else {
        NumericalStatus::NumericalStatusUnknown
    }
}

/// `analyzeControlFlow`.
pub fn analyze_control_flow(params: CfgParameters, t: &Token) -> CfgAnalysis {
    let cfg = build_graph(params, t);
    let (entry, exit) = *cfg
        .id_to_range
        .get(&t.id)
        .expect("ShellCheck internal error, please report: Missing root");
    let env = create_environment_state();
    let mut ctx = Ctx {
        graph: &cfg.graph,
        counter: 1,
        cache: IntMap::default(),
        enable_cache: true,
        invocations: BTreeMap::new(),
        stack: Vec::new(),
        frames: vec![Frame {
            node: entry,
            input: env.clone(),
            output: env.clone(),
        }],
        contexts: IntMap::default(),
    };

    // runRoot
    let (states, frame) = ctx.with_new_stack_frame(entry, false, &mut |c| c.dataflow(entry));
    let deps = frame.dependencies;
    let exit_state = states
        .get(&exit)
        .map(|(_, o)| o.clone())
        .expect("ShellCheck internal error, please report: Missing exit state");
    ctx.register_flow_result(entry, states, deps);

    // Invoke all functions that were declared but not invoked.
    let mut invoked_nodes: BTreeSet<Node> = BTreeSet::new();
    for (_, m) in ctx.invocations.values() {
        invoked_nodes.extend(m.keys().copied());
    }
    let mut declared: BTreeMap<Node, FunctionDefinition> = BTreeMap::new();
    for set in exit_state.function_targets.storage.values() {
        for d in set.iter() {
            if let FunctionDefinition::FunctionDefinition(_, e, _) = d {
                declared.insert(*e, d.clone());
            }
        }
    }
    let mut straggler_input = patch_state(&env, &exit_state);
    straggler_input.exit_codes = None;
    for (e, def) in declared {
        if invoked_nodes.contains(&e) {
            continue;
        }
        let f = ctx.frame();
        f.input = straggler_input.clone();
        f.output = straggler_input.clone();
        f.node = e;
        ctx.transfer_function_value(&def);
    }

    // Round up all the states from all data flows.
    let invocations = std::mem::take(&mut ctx.invocations);
    let mut grouped: BTreeMap<Node, Vec<(InternalState, InternalState)>> = BTreeMap::new();
    for (deps, m) in invocations.into_values() {
        let base = deps_to_state(&deps);
        for (node, (a, b)) in m {
            grouped
                .entry(node)
                .or_default()
                .insert(0, (patch_state(&base, &a), patch_state(&base, &b)));
        }
    }
    let mut invoked_states: BTreeMap<Node, (InternalState, InternalState)> = BTreeMap::new();
    for (node, list) in grouped {
        let (pres, posts): (Vec<InternalState>, Vec<InternalState>) = list.into_iter().unzip();
        let pre = ctx.merge_states(new_internal_state(), &pres);
        let post = ctx.merge_states(new_internal_state(), &posts);
        invoked_states.insert(node, (pre, post));
    }
    let (min, max) = cfg.graph.node_range();
    let mut node_to_data = BTreeMap::new();
    for n in min..=max {
        let pair = invoked_states
            .get(&n)
            .cloned()
            .unwrap_or_else(|| (unreachable_state(), unreachable_state()));
        node_to_data.insert(n, (ProgramState(pair.0), ProgramState(pair.1)));
    }
    drop(ctx);
    CfgAnalysis {
        graph: cfg.graph,
        token_to_range: cfg.id_to_range,
        token_to_nodes: cfg.id_to_nodes,
        post_dominators: cfg.post_dominators,
        node_to_data,
    }
}
