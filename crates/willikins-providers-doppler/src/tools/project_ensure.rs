//! `doppler.project.ensure`: creates a real Doppler project. Port table
//! and behaviour identical to `willikins_providers_fake`'s tool of the
//! same name (`tests/catalog_parity.rs` pins the two `ToolSpec`s equal).

use std::sync::Arc;

use willikins_core::tool::helpers::{
    conflict, exact, get, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::DopplerProject;

use crate::client::{DopplerClient, MANAGED_DESCRIPTION};

/// `doppler.project.ensure`.
pub struct DopplerProjectEnsure {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

impl DopplerProjectEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("project"), exact("DopplerProject", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("project"), scalar("DopplerProject"));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.project.ensure"),
                description: "Ensure a Doppler project exists.".to_string(),
                inputs,
                outputs,
                key: vec![port("project")],
                class: Class::Reversible,
                pure: false,
            },
            client,
        }
    }

    fn outputs_for(project: &DopplerProject) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("project"), Value::known(project.clone()));
        outputs
    }

    /// `GET` the project, mapped to an [`Observation`]. Shared by `read`
    /// and `ensure`.
    ///
    /// The ownership check is exact equality against
    /// [`MANAGED_DESCRIPTION`], not a substring match: an operator who
    /// appends a note to the description (or a project this tool created
    /// whose description was later edited) reads as `Foreign` rather than
    /// `Present`, which is the safer failure mode — an ensure that would
    /// otherwise silently claim a description-edited project as still
    /// ours risks acting on a resource a human has started managing by
    /// hand. Pinned by `tests/project_ensure_mock.rs`.
    fn observe(&self, project: &DopplerProject) -> Result<Observation, ToolError> {
        match self.client.get_project(project) {
            Ok(body) if body.description.as_deref() == Some(MANAGED_DESCRIPTION) => {
                Ok(Observation::Present(Self::outputs_for(project)))
            }
            Ok(_) => Ok(Observation::Foreign),
            Err(err) if err.status == Some(404) => Ok(Observation::Absent {
                predicted: Self::outputs_for(project),
            }),
            Err(err) => Err(err.into()),
        }
    }

    fn foreign_conflict(project: &DopplerProject) -> ToolError {
        conflict(format!("`{project}` already exists and is not ours"))
    }
}

impl Tool for DopplerProjectEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let project: DopplerProject = get(inputs, "project")?;
        self.observe(&project)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let project: DopplerProject = get(inputs, "project")?;
        match self.observe(&project)? {
            Observation::Foreign => Err(Self::foreign_conflict(&project)),
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Mismatch { .. } => unreachable!(
                "doppler.project.ensure's own observe never returns Mismatch: it has no \
                 non-key input to mismatch on"
            ),
            Observation::Absent { .. } => match self.client.create_project(&project) {
                Ok(()) => Ok(Ensured {
                    outputs: Self::outputs_for(&project),
                    changed: true,
                }),
                // The create may have landed despite the error (Doppler
                // documents no error-body schema at all, so this crate
                // cannot tell "already exists" apart from any other
                // failure the way GitHub's `already_exists` flag lets
                // `willikins-providers-github` do): re-read rather than
                // assume either way.
                Err(err) => match self.observe(&project)? {
                    Observation::Present(outputs) => Ok(Ensured {
                        outputs,
                        changed: false,
                    }),
                    Observation::Foreign => Err(Self::foreign_conflict(&project)),
                    Observation::Absent { .. } | Observation::Mismatch { .. } => Err(err.into()),
                },
            },
        }
    }
}
