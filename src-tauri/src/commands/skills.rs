use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::Skill;
use tauri::State;

/// The Ask box's own commands; a skill can't take their names. Must match BUILTINS in
/// src/features/ask/commands.ts.
pub const BUILTIN_COMMANDS: [&str; 5] = ["followup", "new", "explain", "fix", "improve"];
pub const MAX_SLUG: usize = 32;
pub const MAX_DESCRIPTION: usize = 100;
pub const MAX_INSTRUCTION: usize = 1000;

/// Saved skills, in the order they were created.
#[tauri::command]
pub fn list_skills(state: State<'_, AppState>) -> Vec<Skill> {
    state.store.skills()
}

/// Creates a skill, or replaces the one called `original` (its slug before an edit).
#[tauri::command]
pub fn save_skill(state: State<'_, AppState>, skill: Skill, original: Option<String>) -> AppResult<Skill> {
    let mut list = state.store.skills();
    let skill = validate(skill, &list, original.as_deref())?;
    match original.and_then(|o| list.iter().position(|s| s.slug == o)) {
        Some(i) => list[i] = skill.clone(),
        None => list.push(skill.clone()),
    }
    state.store.save_skills(&list)?;
    Ok(skill)
}

#[tauri::command]
pub fn delete_skill(state: State<'_, AppState>, slug: String) -> AppResult<()> {
    let list: Vec<Skill> = state.store.skills().into_iter().filter(|s| s.slug != slug).collect();
    state.store.save_skills(&list)
}

/// The skill tidied up, or what to change. Same rules as `skillError` in the UI.
pub fn validate(skill: Skill, existing: &[Skill], original: Option<&str>) -> AppResult<Skill> {
    let slug = skill.slug.trim().trim_start_matches('/').to_string();
    let description = skill.description.trim().to_string();
    let instruction = skill.instruction.trim().to_string();
    let error = |m: &str| Err(AppError::User(m.to_string()));
    if slug.is_empty() {
        return error("Give the skill a name.");
    }
    if slug.len() > MAX_SLUG {
        return error(&format!("Keep the name to {MAX_SLUG} characters."));
    }
    if !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return error("Use only lowercase letters, numbers and dashes in the name.");
    }
    if BUILTIN_COMMANDS.contains(&slug.as_str()) {
        return error(&format!("/{slug} is a built-in command. Pick another name."));
    }
    if existing.iter().any(|s| s.slug == slug && Some(s.slug.as_str()) != original) {
        return error(&format!("You already have a skill called /{slug}."));
    }
    if description.chars().count() > MAX_DESCRIPTION {
        return error(&format!("Keep the description to {MAX_DESCRIPTION} characters."));
    }
    if instruction.is_empty() {
        return error("Write the instruction.");
    }
    if instruction.chars().count() > MAX_INSTRUCTION {
        return error(&format!("Keep the instruction to {MAX_INSTRUCTION} characters."));
    }
    Ok(Skill { slug, description, instruction })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(slug: &str) -> Skill {
        Skill { slug: slug.into(), description: "".into(), instruction: "Never select email columns.".into() }
    }

    fn err(r: AppResult<Skill>) -> String {
        r.err().expect("should be rejected").to_string()
    }

    #[test]
    fn tidies_a_valid_skill() {
        let s = validate(Skill { slug: " /pii-safe ".into(), description: " No PII ".into(), instruction: " Hide emails. ".into() }, &[], None).unwrap();
        assert_eq!(s, Skill { slug: "pii-safe".into(), description: "No PII".into(), instruction: "Hide emails.".into() });
    }

    #[test]
    fn rejects_bad_names() {
        assert!(err(validate(skill(""), &[], None)).contains("name"));
        assert!(err(validate(skill("PII Safe"), &[], None)).contains("lowercase"));
        assert!(err(validate(skill(&"a".repeat(MAX_SLUG + 1)), &[], None)).contains("32"));
        assert!(err(validate(skill("explain"), &[], None)).contains("built-in"));
    }

    #[test]
    fn names_are_unique_except_for_the_skill_being_edited() {
        let existing = [skill("pii-safe")];
        assert!(err(validate(skill("pii-safe"), &existing, None)).contains("already"));
        assert!(validate(skill("pii-safe"), &existing, Some("pii-safe")).is_ok());
        assert!(err(validate(skill("pii-safe"), &[skill("pii-safe"), skill("other")], Some("other"))).contains("already"));
    }

    #[test]
    fn needs_a_short_instruction() {
        let mut s = skill("x");
        s.instruction = "  ".into();
        assert!(err(validate(s.clone(), &[], None)).contains("instruction"));
        s.instruction = "a".repeat(MAX_INSTRUCTION + 1);
        assert!(err(validate(s, &[], None)).contains("1000"));
    }
}
