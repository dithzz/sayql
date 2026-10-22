import { useCallback, useEffect, useState } from "react";
import { commands, type Skill } from "../../api";

/** Saved skills, in the order they were created. */
export function useSkills() {
  const [skills, setSkills] = useState<Skill[]>([]);

  useEffect(() => {
    commands.listSkills().then(setSkills, () => setSkills([]));
  }, []);

  /** Creates, or replaces the skill whose slug was `original`; throws with what to change. */
  const save = useCallback(async (skill: Skill, original: string | null) => {
    const saved = await commands.saveSkill(skill, original);
    setSkills((all) => {
      const i = original === null ? -1 : all.findIndex((s) => s.slug === original);
      return i < 0 ? [...all, saved] : all.map((s, j) => (j === i ? saved : s));
    });
    return saved;
  }, []);

  const remove = useCallback(async (slug: string) => {
    await commands.deleteSkill(slug);
    setSkills((all) => all.filter((s) => s.slug !== slug));
  }, []);

  return { skills, save, remove };
}

export type SkillsApi = ReturnType<typeof useSkills>;
