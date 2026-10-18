//! A body renamed on the evaluated model (`body.rename`). A name is no geometry: the features
//! after the one that made the body keep their results under the new name, where evaluating
//! them again took minutes on a design with a large pattern.
//!
//! The results are renamed only where evaluating the renamed design gives the same results
//! under the new name; anywhere a name could steer more than a lookup (a name derived from
//! the old one, text that spells it, a counted name freed for a later body) the design is
//! evaluated again.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde_json::Value as Json;
use solidcraft_sketch::LinkSource;

use super::{
    CachedEval, FeatureResult, Model, ModelState, SolvedSketch, StepDigests, cache_key, chain_fingerprint, context_digest, evaluation_values,
    feature_json, fingerprint, placement_digest,
};
use crate::expr::Value;
use crate::naming::NameCell;
use crate::{Document, Feature, FeatureKind};

/// Nesting followed in a definition or a link (deeper: evaluated again instead).
const DEPTH: usize = 64;

/// Body `old`, made by feature `creator`, called `new` in `after` (`before` renamed).
struct Rename<'a> {
    before: &'a Document,
    after: &'a Document,
    creator: u64,
    old: &'a str,
    new: &'a str,
}

impl Model {
    /// Body `old`, made by feature `creator`, is `new` in `after` (`before` with `body.rename`
    /// applied). Rename it on the results from `creator` on and fingerprint them for `after`, so
    /// the next [`Model::evaluate`] reuses them; they are cached for `after` too. `false`,
    /// changing nothing, where evaluating `after` could give more than a new name.
    pub fn rename_body(&mut self, before: &Document, after: &Document, creator: u64, old: &str, new: &str) -> bool {
        let Some((results, cached)) = self.renamed(&Rename { before, after, creator, old, new }) else { return false };
        self.results = results;
        self.step_digests = StepDigests::of(after, &self.step_digests);
        if let Ok(mut cache) = self.cache.lock() {
            for (key, e) in cached {
                cache.put(key, e);
            }
        }
        true
    }

    fn renamed(&self, r: &Rename) -> Option<(Vec<FeatureResult>, Vec<(u64, CachedEval)>)> {
        let Rename { before, after, creator, old, new } = *r;
        if self.no_cache || self.is_stale() || old == new || counted_name(new) {
            return None;
        }
        // Evaluating puts a body named `new` in the component (and offset) filed under it.
        if before.body_components.contains_key(new) || before.body_offsets.contains_key(new) {
            return None;
        }
        let n = self.results.len();
        let aligned = before.features.len() == n
            && after.features.len() == n
            && self.results.iter().zip(&before.features).zip(&after.features).all(|((r, b), a)| r.id == b.id && r.id == a.id);
        let c = after.features.iter().position(|f| f.id == creator).filter(|_| aligned)?;
        let vals = evaluation_values(after).0;
        let steps = StepDigests::of(after, &self.step_digests);
        let (fps_before, fps) = (chain(before, &evaluation_values(before).0, &steps), chain(after, &vals, &steps));
        // The results are `before`'s, and nothing before the body's feature changed.
        if self.results.iter().zip(&fps_before).any(|(r, fp)| r.fingerprint != *fp) || fps_before.get(..c) != fps.get(..c) {
            return None;
        }
        if !self.results.iter().take(c).all(|r| r.state.body(old).is_none() && r.state.body(new).is_none()) {
            return None;
        }
        let counters = self.renamed_counters(r, c, &steps)?;
        let mut states: HashMap<*const ModelState, Arc<ModelState>> = HashMap::new();
        let mut sketches: HashMap<*const SolvedSketch, Arc<SolvedSketch>> = HashMap::new();
        let ctx = context_digest(after, &vals);
        let mut results = self.results.clone();
        let mut cached = Vec::new();
        let mut prev = self.state_after(c.checked_sub(1))?;
        for (i, (res, f)) in self.results.iter().zip(&after.features).enumerate().skip(c) {
            let state = match states.get(&Arc::as_ptr(&res.state)) {
                Some(s) => s.clone(),
                None => {
                    let s = Arc::new(rename_state(&res.state, old, new, *counters.get(i)?, &mut sketches)?);
                    states.insert(Arc::as_ptr(&res.state), s.clone());
                    s
                }
            };
            let fp = *fps.get(i)?;
            let (error, warning) = (renamed_text(&res.error, old, new)?, renamed_text(&res.warning, old, new)?);
            if !res.skipped {
                let local = fingerprint(0, f, &vals, false, &steps) ^ placement_digest(after, f);
                let e = CachedEval {
                    error: error.clone(),
                    warning: warning.clone(),
                    boolean_diagnostics: res.boolean_diagnostics.clone(),
                    ms: res.ms,
                    bytes: state.retained_bytes(),
                    state: state.clone(),
                };
                cached.push((cache_key(f, fp, local, ctx, &prev), e));
            }
            *results.get_mut(i)? = FeatureResult { error, warning, fingerprint: fp, state: state.clone(), ..res.clone() };
            prev = state;
        }
        Some((results, cached))
    }

    /// The state after result `i` (`None`: the empty model).
    fn state_after(&self, i: Option<usize>) -> Option<Arc<ModelState>> {
        match i {
            Some(i) => self.results.get(i).map(|r| r.state.clone()),
            None => Some(self.empty.clone()),
        }
    }

    /// The body counter after each feature once the body is renamed, checking that every
    /// feature from the body's (index `c`) on reads the new name where it read the old one and
    /// names its bodies as before. A feature whose stored names changed (`body.rename` stores
    /// the names they have now) takes them as stored and counts no body: the counted names it
    /// took are given back, and no later feature may take a counted name.
    fn renamed_counters(&self, r: &Rename, c: usize, steps: &StepDigests) -> Option<Vec<usize>> {
        let Rename { before, after, creator, old, new } = *r;
        let mut counters: Vec<usize> = self.results.iter().map(|r| r.state.body_counter).collect();
        let mut given_back = 0;
        let mut last = self.state_after(c.checked_sub(1))?.body_counter;
        let json = |f: &Feature| serde_json::from_str::<Json>(&feature_json(f, steps)).ok();
        for (i, ((res, b), a)) in self.results.iter().zip(&before.features).zip(&after.features).enumerate().skip(c) {
            let (Some(mut jb), Some(mut ja)) = (json(b), json(a)) else { return None };
            if any_string(&ja, 0, &mut |s| s != new && (mentions(s, old) || mentions(s, new)))
                || any_string(&jb, 0, &mut |s| s != old && mentions(s, new))
            {
                return None;
            }
            for v in [&mut jb, &mut ja] {
                if let Some(o) = v.as_object_mut() {
                    o.remove("body_names");
                }
            }
            if !rename_strings(&mut jb, old, new, 0) || jb != ja {
                return None;
            }
            for body in &res.state.bodies {
                if (body.name != old && mentions(&body.name, new))
                    || (mentions(&body.name, old) && (body.name != old || body.feature != creator || NameCell::from_body_name(body)))
                    || body.source_name.as_deref().is_some_and(|s| mentions(s, old) || mentions(s, new))
                {
                    return None;
                }
            }
            let resolved = |name: &Option<String>| name.as_deref().is_some_and(|n| mentions(n, old));
            if res.state.sketches.iter().flat_map(|s| &s.sketch.links).any(|l| resolved(&l.face_name) || resolved(&l.edge_name)) {
                return None;
            }
            let counted = res.state.body_counter.checked_sub(last)?;
            last = res.state.body_counter;
            if b.body_names != a.body_names {
                if matches!(&a.kind, FeatureKind::Import { skip, .. } if !skip.is_empty()) {
                    return None;
                }
                let made: Vec<&str> = if res.skipped || res.error.is_some() {
                    Vec::new()
                } else {
                    res.state.bodies.iter().filter(|x| x.feature == res.id).map(|x| if x.name == old { new } else { x.name.as_str() }).collect()
                };
                let stored = |k: usize| a.body_names.get(k).map(String::as_str);
                let rest_kept =
                    (made.len()..a.body_names.len().max(b.body_names.len())).all(|k| stored(k) == b.body_names.get(k).map(String::as_str));
                if made.iter().enumerate().any(|(k, m)| stored(k) != Some(*m)) || !rest_kept {
                    return None;
                }
                given_back += counted;
            } else if given_back > 0 && counted > 0 {
                return None;
            }
            *counters.get_mut(i)? = res.state.body_counter.checked_sub(given_back)?;
        }
        Some(counters)
    }
}

/// The chain fingerprints of `doc`'s features, as [`Model::evaluate`] takes them.
fn chain(doc: &Document, vals: &BTreeMap<String, Value>, steps: &StepDigests) -> Vec<u64> {
    let marker = doc.marker.unwrap_or(usize::MAX);
    let mut prev = 0;
    doc.features
        .iter()
        .enumerate()
        .map(|(i, f)| {
            prev = chain_fingerprint(doc, vals, f, prev, i >= marker, steps);
            prev
        })
        .collect()
}

/// `st` with the body called `new`, and `counter` as its body counter. `None` where a link is
/// nested too deep to rename.
fn rename_state(
    st: &ModelState,
    old: &str,
    new: &str,
    counter: usize,
    sketches: &mut HashMap<*const SolvedSketch, Arc<SolvedSketch>>,
) -> Option<ModelState> {
    let mut s = st.clone();
    for b in s.bodies.iter_mut().filter(|b| b.name == old) {
        b.names = NameCell::renamed(b);
        b.name = new.to_string();
    }
    for t in s.threads.iter_mut().filter(|t| t.body == old) {
        t.body = new.to_string();
    }
    for sheet in s.sheets.iter_mut().filter(|sheet| sheet.body == old) {
        sheet.body = new.to_string();
    }
    for sk in &mut s.sketches {
        if let Some(done) = sketches.get(&Arc::as_ptr(sk)) {
            *sk = done.clone();
            continue;
        }
        let mut renamed = (**sk).clone();
        let mut changed = false;
        for link in &mut renamed.sketch.links {
            changed |= rename_source(&mut link.source, old, new, 0)?;
        }
        let renamed = if changed { Arc::new(renamed) } else { sk.clone() };
        sketches.insert(Arc::as_ptr(sk), renamed.clone());
        *sk = renamed;
    }
    s.body_counter = counter;
    Some(s)
}

/// Rename body `old` in a link's source. `None` when nested too deep.
fn rename_source(source: &mut LinkSource, old: &str, new: &str, depth: usize) -> Option<bool> {
    match source {
        LinkSource::Edge { body, .. }
        | LinkSource::Face { body, .. }
        | LinkSource::Body { body }
        | LinkSource::Vertex { body, .. }
        | LinkSource::OnSurface { body, .. }
        | LinkSource::Iso { body, .. }
        | LinkSource::Spun { body, .. } => {
            let hit = body == old;
            if hit {
                *body = new.to_string();
            }
            Some(hit)
        }
        LinkSource::Intersection { a, b } if depth < DEPTH => {
            let a = rename_source(a, old, new, depth + 1)?;
            Some(rename_source(b, old, new, depth + 1)? | a)
        }
        LinkSource::Intersection { .. } => None,
        LinkSource::SketchCurve { .. }
        | LinkSource::SketchPoint { .. }
        | LinkSource::Origin
        | LinkSource::Axis { .. }
        | LinkSource::SketchAxis { .. }
        | LinkSource::Plane { .. }
        | LinkSource::Text { .. } => Some(false),
    }
}

/// A result's message under the new name: `None` where it names the body other than as
/// `` `old` ``, or names `new` already (evaluating again words it).
fn renamed_text(text: &Option<String>, old: &str, new: &str) -> Option<Option<String>> {
    let Some(text) = text else { return Some(None) };
    let out = text.replace(&format!("`{old}`"), &format!("`{new}`"));
    (!mentions(text, new) && !mentions(&out, old)).then_some(Some(out))
}

/// A name the body counter gives (`Body7`): a later counted body could take it.
fn counted_name(name: &str) -> bool {
    name.strip_prefix("Body").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// `name` as a word of `s` (`Body1` in `Body1 (2)` or `Body1:face:3`, not in `Body10`).
fn mentions(s: &str, name: &str) -> bool {
    !name.is_empty()
        && s.match_indices(name).any(|(i, _)| {
            let before = s.get(..i).and_then(|x| x.chars().next_back());
            let after = s.get(i + name.len()..).and_then(|x| x.chars().next());
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        })
}

/// Whether `f` holds for a string in `v` (or `v` nests too deep to tell).
fn any_string(v: &Json, depth: usize, f: &mut dyn FnMut(&str) -> bool) -> bool {
    match v {
        _ if depth > DEPTH => true,
        Json::String(s) => f(s),
        Json::Array(a) => a.iter().any(|x| any_string(x, depth + 1, f)),
        Json::Object(o) => o.iter().any(|(k, x)| f(k) || any_string(x, depth + 1, f)),
        _ => false,
    }
}

/// Every string `old` in `v` made `new`. `false` when `v` nests too deep.
fn rename_strings(v: &mut Json, old: &str, new: &str, depth: usize) -> bool {
    match v {
        _ if depth > DEPTH => false,
        Json::String(s) => {
            if s == old {
                *s = new.to_string();
            }
            true
        }
        Json::Array(a) => a.iter_mut().all(|x| rename_strings(x, old, new, depth + 1)),
        Json::Object(o) => o.values_mut().all(|x| rename_strings(x, old, new, depth + 1)),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{counted_name, mentions, renamed_text};

    #[test]
    fn a_name_is_mentioned_as_a_word() {
        assert!(mentions("Body1", "Body1"));
        assert!(mentions("Body1 (2)", "Body1"));
        assert!(mentions("Body1:face:3", "Body1"));
        assert!(!mentions("Body10", "Body1"));
        assert!(!mentions("MyBody1", "Body1"));
        assert!(!mentions("F33:top", "Body1"));
        assert!(!mentions("anything", ""));
        assert!(counted_name("Body7") && !counted_name("Body") && !counted_name("Body7a") && !counted_name("Lid"));
    }

    #[test]
    fn a_message_is_renamed_only_where_it_names_the_body_in_backticks() {
        let text = |t: &str| Some(t.to_string());
        assert_eq!(renamed_text(&None, "Body1", "Lid"), Some(None));
        assert_eq!(renamed_text(&text("no edge of `Body1` there"), "Body1", "Lid"), Some(text("no edge of `Lid` there")));
        assert_eq!(renamed_text(&text("Split Face: no face of Body1 at 1, 2, 3"), "Body1", "Lid"), None);
        assert_eq!(renamed_text(&text("`Lid` is in the way"), "Body1", "Lid"), None);
    }
}
