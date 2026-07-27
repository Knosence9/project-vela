use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

/// An opaque, non-blank stable identifier for one registered skill.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SkillId(String);

impl SkillId {
    pub fn new(value: impl Into<String>) -> Result<Self, SkillIdError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(SkillIdError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SkillId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SkillIdError;

impl fmt::Display for SkillIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("skill id must not be blank")
    }
}

impl Error for SkillIdError {}

/// Immutable exact instruction text owned by one process-local registered skill.
#[derive(Clone, Eq, PartialEq)]
pub struct RegisteredSkill {
    id: SkillId,
    instructions: String,
}

impl RegisteredSkill {
    pub fn new(id: SkillId, instructions: impl Into<String>) -> Self {
        Self {
            id,
            instructions: instructions.into(),
        }
    }

    pub fn id(&self) -> &SkillId {
        &self.id
    }

    pub fn instructions(&self) -> &str {
        &self.instructions
    }
}

impl fmt::Debug for RegisteredSkill {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegisteredSkill")
            .field("id", &self.id)
            .field("instructions_len", &self.instructions.len())
            .finish()
    }
}

/// A duplicate exact skill identity rejected during atomic registration.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SkillRegistryError {
    DuplicateId { skill_id: SkillId },
}

impl fmt::Display for SkillRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId { skill_id } => {
                write!(formatter, "skill {skill_id} is already registered")
            }
        }
    }
}

impl Error for SkillRegistryError {}

/// A caller-owned, process-local deterministic directory of inert skill instructions.
#[derive(Debug, Default)]
pub struct SkillRegistry {
    skills: BTreeMap<SkillId, RegisteredSkill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one homogeneous batch atomically without replacing existing instructions.
    pub fn register_all<I>(&mut self, skills: I) -> Result<(), SkillRegistryError>
    where
        I: IntoIterator<Item = RegisteredSkill>,
    {
        let skills = skills.into_iter().collect::<Vec<_>>();
        let mut batch_ids = BTreeSet::new();
        for skill in &skills {
            let skill_id = skill.id().clone();
            if self.skills.contains_key(&skill_id) || !batch_ids.insert(skill_id.clone()) {
                return Err(SkillRegistryError::DuplicateId { skill_id });
            }
        }
        for skill in skills {
            self.skills.insert(skill.id().clone(), skill);
        }
        Ok(())
    }

    pub fn get(&self, skill_id: &SkillId) -> Option<&RegisteredSkill> {
        self.skills.get(skill_id)
    }

    /// Iterates registered skills in ascending exact-ID order.
    pub fn skills(&self) -> impl Iterator<Item = &RegisteredSkill> {
        self.skills.values()
    }
}
