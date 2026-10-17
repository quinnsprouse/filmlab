import { extractProfile } from "@/lib/engine/engine";
import type { DevelopSettings, FilmSettings } from "@/lib/engine/types";
import { readValue, writeValue } from "@/lib/storage/kv";
import { createStore } from "@/lib/store";

export interface Profile {
  id: string;
  name: string;
  groups: string[];
  partial: DevelopSettings;
  film?: FilmSettings;
  createdAt: string;
}

const key = "profiles";

export const profiles = createStore<Profile[]>([]);

function isProfile(value: unknown): value is Profile {
  return (
    typeof value === "object" &&
    value !== null &&
    "id" in value &&
    typeof value.id === "string" &&
    "name" in value &&
    typeof value.name === "string" &&
    "partial" in value &&
    typeof value.partial === "object"
  );
}

export async function loadProfiles(): Promise<void> {
  const stored = await readValue(key);
  profiles.set(Array.isArray(stored) ? stored.filter(isProfile) : []);
}

export async function saveProfile(
  name: string,
  settings: DevelopSettings,
  groups: string[],
  film?: FilmSettings,
): Promise<Profile> {
  const base = {
    id: crypto.randomUUID(),
    name: name.trim(),
    groups,
    partial: extractProfile(
      settings,
      groups.filter((group) => group !== "film"),
    ),
    createdAt: new Date().toISOString(),
  };
  const profile: Profile = film && groups.includes("film") ? { ...base, film } : base;
  const next = [profile, ...profiles.get()];
  profiles.set(next);
  await writeValue(key, next);
  return profile;
}

export async function deleteProfile(id: string): Promise<void> {
  const next = profiles.get().filter((profile) => profile.id !== id);
  profiles.set(next);
  await writeValue(key, next);
}
