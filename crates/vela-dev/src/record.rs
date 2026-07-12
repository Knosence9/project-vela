use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaVersion;

impl Serialize for SchemaVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(1)
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u64::deserialize(deserializer)? {
            1 => Ok(Self),
            version => Err(de::Error::custom(format!(
                "unsupported schema version {version}; expected 1"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentRecord {
    pub schema_version: SchemaVersion,
    pub task: TaskContext,
    pub attempts: Vec<Attempt>,
    pub outcome: Outcome,
    pub lessons: Vec<String>,
    pub provenance: Provenance,
    pub sanitation: Sanitation,
    pub trust: Trust,
    pub example: Example,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskContext {
    pub title: String,
    pub objective: String,
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub summary: String,
    pub outcome: AttemptOutcome,
    pub diagnostic: Option<String>,
    pub patch: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    Success,
    Failure,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub summary: String,
    pub verified: bool,
    pub verification: Vec<Verification>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verification {
    pub command: String,
    pub status: VerificationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Passed,
    Failed,
    NotRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub repository_path: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sanitation {
    pub passed: bool,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trust {
    Untrusted,
    Reviewed,
    Curated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Example {
    #[serde(rename = "type")]
    pub kind: ExampleType,
    pub rejection_rationale: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExampleType {
    Positive,
    Negative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub code: &'static str,
    pub path: String,
    pub message: &'static str,
}

impl ValidationIssue {
    #[must_use]
    pub fn new(code: &'static str, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code,
            path: path.into(),
            message,
        }
    }
}

impl DevelopmentRecord {
    #[must_use]
    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        required(&mut issues, "task.title", &self.task.title);
        required(&mut issues, "task.objective", &self.task.objective);
        if self.task.acceptance_criteria.is_empty()
            || self
                .task
                .acceptance_criteria
                .iter()
                .any(|item| item.trim().is_empty())
        {
            issues.push(ValidationIssue::new(
                "required",
                "task.acceptance_criteria",
                "must contain non-empty criteria",
            ));
        }

        let has_pass = self
            .outcome
            .verification
            .iter()
            .any(|check| check.status == VerificationStatus::Passed);
        let final_verification_failed = self
            .outcome
            .verification
            .last()
            .is_some_and(|check| check.status == VerificationStatus::Failed);
        if self.outcome.verified && !has_pass {
            issues.push(ValidationIssue::new(
                "verified_without_pass",
                "outcome.verification",
                "verified outcomes require a passing verification",
            ));
        }
        if self.trust == Trust::Curated && final_verification_failed {
            issues.push(ValidationIssue::new(
                "curated_after_failed_verification",
                "trust",
                "curated trust requires final verification not to fail",
            ));
        }
        let has_diagnostic = self
            .attempts
            .iter()
            .any(|attempt| nonempty(&attempt.diagnostic));
        if self.example.kind == ExampleType::Negative
            && !has_diagnostic
            && !nonempty(&self.example.rejection_rationale)
        {
            issues.push(ValidationIssue::new(
                "negative_without_diagnostic",
                "example.type",
                "negative examples require a diagnostic or rejection rationale",
            ));
        }
        if is_absolute(&self.provenance.repository_path) {
            issues.push(ValidationIssue::new(
                "absolute_repository_path",
                "provenance.repository_path",
                "repository paths must be relative",
            ));
        }
        if has_parent_component(&self.provenance.repository_path) {
            issues.push(ValidationIssue::new(
                "repository_path_traversal",
                "provenance.repository_path",
                "repository paths must not contain parent traversal",
            ));
        }
        if !is_https_url(&self.provenance.url) {
            issues.push(ValidationIssue::new(
                "insecure_provenance_url",
                "provenance.url",
                "provenance URLs must use HTTPS",
            ));
        }
        if self.sanitation.passed && !self.sanitation.blockers.is_empty() {
            issues.push(ValidationIssue::new(
                "sanitation_blocked",
                "sanitation.passed",
                "sanitation cannot pass while blockers remain",
            ));
        }

        self.inspect_text(&mut issues);
        issues
    }

    fn inspect_text(&self, issues: &mut Vec<ValidationIssue>) {
        inspect(issues, "task.title", &self.task.title);
        inspect(issues, "task.objective", &self.task.objective);
        for (index, criterion) in self.task.acceptance_criteria.iter().enumerate() {
            inspect(
                issues,
                &format!("task.acceptance_criteria[{index}]"),
                criterion,
            );
        }
        for (index, attempt) in self.attempts.iter().enumerate() {
            inspect(
                issues,
                &format!("attempts[{index}].summary"),
                &attempt.summary,
            );
            if let Some(diagnostic) = &attempt.diagnostic {
                inspect(issues, &format!("attempts[{index}].diagnostic"), diagnostic);
            }
            inspect(issues, &format!("attempts[{index}].patch"), &attempt.patch);
        }
        inspect(issues, "outcome.summary", &self.outcome.summary);
        for (index, verification) in self.outcome.verification.iter().enumerate() {
            inspect(
                issues,
                &format!("outcome.verification[{index}].command"),
                &verification.command,
            );
        }
        for (index, lesson) in self.lessons.iter().enumerate() {
            inspect(issues, &format!("lessons[{index}]"), lesson);
        }
        inspect(
            issues,
            "provenance.repository_path",
            &self.provenance.repository_path,
        );
        inspect(issues, "provenance.url", &self.provenance.url);
        for (index, blocker) in self.sanitation.blockers.iter().enumerate() {
            inspect(issues, &format!("sanitation.blockers[{index}]"), blocker);
        }
        if let Some(rationale) = &self.example.rejection_rationale {
            inspect(issues, "example.rejection_rationale", rationale);
        }
    }
}

fn required(issues: &mut Vec<ValidationIssue>, path: &'static str, value: &str) {
    if value.trim().is_empty() {
        issues.push(ValidationIssue::new("required", path, "must not be empty"));
    }
}

fn nonempty(value: &Option<String>) -> bool {
    value.as_ref().is_some_and(|text| !text.trim().is_empty())
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with('\\')
        || path.as_bytes().get(1).is_some_and(|byte| *byte == b':')
}

fn has_parent_component(path: &str) -> bool {
    path.split(['/', '\\']).any(|component| component == "..")
}

fn is_https_url(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|remainder| {
        let authority = remainder.split('/').next().unwrap_or_default();
        !authority.is_empty() && !authority.chars().any(char::is_whitespace)
    })
}

fn inspect(issues: &mut Vec<ValidationIssue>, path: &str, value: &str) {
    let lower = value.to_ascii_lowercase();
    if [
        "api_key",
        "apikey",
        "secret=",
        "secret =",
        "password=",
        "password =",
        "sk-",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        issues.push(ValidationIssue::new(
            "secret_detected",
            path,
            "obvious secret detected",
        ));
    }
    let normalized = lower.replace('\\', "/");
    if normalized.contains("/home/") || normalized.contains("/users/") {
        issues.push(ValidationIssue::new(
            "home_path_detected",
            path,
            "absolute home-directory path detected",
        ));
    }
}
