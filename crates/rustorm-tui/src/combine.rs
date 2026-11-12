//! The Combine view's state: the files picked for a merge, the output, the
//! per-host decisions and the core's preview (docs/cli.md, combine).

use std::path::{Path, PathBuf};

use rustorm_core::{
    CombinePreview, CombineRequest, HostDecision, HostDecisions, OnConflict, Workspace,
};

/// Which step the view is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Picking the base, the inputs and the output.
    Files,
    /// The merge as it would be written, conflicts decided one by one.
    Preview,
}

/// A one-line text entry open over the file step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Entry {
    /// `t`: a path to add to the list.
    AddPath(String),
    /// `o`: the output path; empty means the base.
    Output(String),
}

/// The Combine view.
#[derive(Debug, Clone)]
pub(crate) struct Combine {
    /// Every workspace file in load order, then each typed path.
    pub candidates: Vec<PathBuf>,
    /// Indices into `candidates`, base first, in pick order.
    pub chosen: Vec<usize>,
    /// The highlighted candidate.
    pub sel: usize,
    /// Where the result goes; `None` writes the base.
    pub output: Option<PathBuf>,
    /// Move each later input an `Include` loads to `~/.ssh/retired/`.
    pub retire: bool,
    /// `k` and `r` so far, by conflict name.
    pub decisions: HostDecisions,
    /// The last preview read.
    pub preview: Option<CombinePreview>,
    /// The highlighted conflict of the preview.
    pub conflict_sel: usize,
    /// First line of the merged text on screen.
    pub scroll: u16,
    pub step: Step,
    pub entry: Option<Entry>,
    /// The view's key overlay is open.
    pub help: bool,
}

impl Combine {
    /// The view over `ws`: every loaded file listed, the root picked as
    /// the base.
    pub fn new(ws: &Workspace) -> Combine {
        Combine {
            candidates: ws.files.iter().map(|f| f.path.clone()).collect(),
            chosen: vec![0],
            sel: 0,
            output: None,
            retire: false,
            decisions: HostDecisions::new(),
            preview: None,
            conflict_sel: 0,
            scroll: 0,
            step: Step::Files,
            entry: None,
            help: false,
        }
    }

    /// The core request the choices describe, as the CLI flags would.
    pub fn request(&self) -> CombineRequest {
        CombineRequest {
            inputs: self
                .chosen
                .iter()
                .map(|&i| self.candidates[i].clone())
                .collect(),
            output: self.output.clone(),
            policy: OnConflict::Fail,
            decisions: self.decisions.clone(),
            retire: self.retire,
        }
    }

    /// `base`, `input 1`, ... or empty for candidate `i`.
    pub fn role(&self, i: usize) -> String {
        match self.chosen.iter().position(|&c| c == i) {
            Some(0) => "base".into(),
            Some(n) => format!("input {n}"),
            None => String::new(),
        }
    }

    /// `Space`: picks candidate `sel` as the next input, or drops it.
    pub fn toggle(&mut self) {
        if let Some(at) = self.chosen.iter().position(|&c| c == self.sel) {
            self.chosen.remove(at);
        } else if self.sel < self.candidates.len() {
            self.chosen.push(self.sel);
        }
    }

    /// `b`: makes candidate `sel` the base.
    pub fn make_base(&mut self) {
        self.chosen.retain(|&c| c != self.sel);
        self.chosen.insert(0, self.sel);
    }

    /// Adds `path` to the list and picks it; a path already listed is
    /// picked where it is.
    pub fn add_path(&mut self, path: PathBuf) {
        let i = match self.candidates.iter().position(|c| c == &path) {
            Some(i) => i,
            None => {
                self.candidates.push(path);
                self.candidates.len() - 1
            }
        };
        if !self.chosen.contains(&i) {
            self.chosen.push(i);
        }
        self.sel = i;
    }

    /// The decision in effect for the highlighted conflict is set to `d`.
    pub fn decide(&mut self, d: HostDecision) -> Option<String> {
        let name = self
            .preview
            .as_ref()?
            .conflicts
            .get(self.conflict_sel)?
            .name
            .clone();
        self.decisions.insert(name.clone(), d);
        Some(name)
    }

    /// The file the result goes to.
    pub fn output_path(&self) -> Option<PathBuf> {
        self.request().output_path().map(Path::to_path_buf)
    }
}

/// `~/x` against `home`, else `text` as a path.
pub(crate) fn expand(text: &str, home: Option<&Path>) -> PathBuf {
    match (text.strip_prefix("~/"), home) {
        (Some(rest), Some(h)) => h.join(rest),
        _ if text == "~" => home.map_or_else(|| PathBuf::from(text), Path::to_path_buf),
        _ => PathBuf::from(text),
    }
}
