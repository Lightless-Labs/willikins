//! `naming.v1`: wraps `willikins_types::naming::v1`. Pure: no key, no
//! state, `ensure` is the identity of `read`.

use indexmap::IndexMap;

use willikins_core::tool::helpers::{exact, get, port, require_present, scalar, tool_name};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::naming;

/// `naming.v1`.
pub struct NamingV1 {
    spec: ToolSpec,
}

impl NamingV1 {
    /// Build the tool, constructing its spec.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("org"), exact("GitHubOrg", true));
        inputs.insert(port("slug"), exact("ProjectSlug", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("github_repo"), scalar("GitHubRepo"));
        outputs.insert(port("doppler_project"), scalar("DopplerProject"));
        Self {
            spec: ToolSpec {
                name: tool_name("naming.v1"),
                description: "Derive this project's GitHub repository and Doppler project identities from its org and slug.".to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
        }
    }

    /// Compute both derived identities from `inputs`. Shared by `read` and
    /// `ensure`, since a pure tool's `ensure` is the identity of its
    /// `read`.
    fn compute(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let org = get(inputs, "org")?;
        let slug = get(inputs, "slug")?;
        let github_repo = naming::v1::github_repo(&org, &slug);
        let doppler_project = naming::v1::doppler_project(&slug);
        let mut outputs = Outputs::new();
        outputs.insert(port("github_repo"), Value::known(github_repo));
        outputs.insert(port("doppler_project"), Value::known(doppler_project));
        Ok(outputs)
    }
}

impl Default for NamingV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for NamingV1 {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.compute(inputs).map(Observation::Present)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: self.compute(inputs)?,
            changed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_types::{DomainType, GitHubOrg, ProjectSlug};

    fn tool() -> NamingV1 {
        NamingV1::new()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("org").unwrap(),
            Value::known(GitHubOrg::parse("lightless-labs").unwrap()),
        );
        inputs.insert(
            PortName::parse("slug").unwrap(),
            Value::known(ProjectSlug::parse("third-thoughts").unwrap()),
        );
        inputs
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_derives_both_identities() {
        let observation = tool().read(&full_inputs()).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("naming.v1 always reports Present");
        };
        let repo = outputs
            .get(&PortName::parse("github_repo").unwrap())
            .unwrap();
        assert_eq!(repo.render().to_string(), "lightless-labs/third-thoughts");
        let project = outputs
            .get(&PortName::parse("doppler_project").unwrap())
            .unwrap();
        assert_eq!(project.render().to_string(), "third-thoughts");
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("org").unwrap(),
            Value::known(GitHubOrg::parse("lightless-labs").unwrap()),
        );
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("slug"), "{}", err.message);
    }

    #[test]
    fn read_rejects_an_unknown_input() {
        let mut inputs = full_inputs();
        inputs.insert(
            PortName::parse("slug").unwrap(),
            Value::unknown(willikins_core::TypeRef::scalar(
                willikins_core::TypeName::parse("ProjectSlug").unwrap(),
            )),
        );
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("slug"), "{}", err.message);
    }

    #[test]
    fn ensure_agrees_with_read() {
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        let token = SinkToken::new();
        let via_ensure = tool().ensure(&full_inputs(), &token).unwrap();
        assert!(
            !via_ensure.changed,
            "a pure tool's ensure never changes anything"
        );
        let Observation::Present(via_read) = tool().read(&full_inputs()).unwrap() else {
            panic!("naming.v1 always reports Present");
        };
        assert_eq!(
            via_ensure
                .outputs
                .get(&PortName::parse("github_repo").unwrap())
                .unwrap(),
            via_read
                .get(&PortName::parse("github_repo").unwrap())
                .unwrap()
        );
    }
}
