import { Slider } from "dialkit";
import { useEffect, useState } from "react";

import { Delete02Icon, Tick02Icon, Icon } from "@/components/icons";
import { useStore } from "@/hooks/use-store";
import { applyProfileTo, develop } from "@/lib/develop/session";
import { photoRecord } from "@/lib/library/library";
import { deleteProfile, profiles, type Profile } from "@/lib/profiles/profiles";
import { cn } from "@/lib/utils";

import { SaveProfile } from "./save-profile";

interface RowProps {
  profile: Profile;
  active: boolean;
  thumbnail: string | undefined;
  onApply: (profile: Profile) => void;
}

const confirmWindowMs = 3000;

function DeleteButton({ profile }: { profile: Profile }) {
  const [confirming, setConfirming] = useState(false);

  useEffect(() => {
    if (!confirming) {
      return;
    }
    const timer = setTimeout(() => setConfirming(false), confirmWindowMs);
    return () => clearTimeout(timer);
  }, [confirming]);

  return (
    <button
      type="button"
      aria-label={confirming ? `Confirm delete ${profile.name}` : `Delete profile ${profile.name}`}
      onClick={() => {
        if (confirming) {
          void deleteProfile(profile.id);
        } else {
          setConfirming(true);
        }
      }}
      className={cn(
        "absolute top-1/2 right-xs grid h-6 -translate-y-1/2 place-items-center rounded-s text-xs font-medium text-secondary-text transition-colors duration-(--motion-duration) ease-(--motion-easing) group-hover:opacity-100 hover:bg-control-hover hover:text-foreground focus-visible:opacity-100",
        confirming ? "bg-control-active px-xs opacity-100" : "w-6 opacity-0",
      )}
    >
      {confirming ? "Delete" : <Icon icon={Delete02Icon} size={14} />}
    </button>
  );
}

function ProfileRow({ profile, active, thumbnail, onApply }: RowProps) {
  return (
    <div className="group relative shrink-0">
      <button
        type="button"
        aria-pressed={active}
        onClick={() => onApply(profile)}
        className={cn(
          "flex h-[52px] w-full items-center gap-s rounded-m pr-s pl-s text-left transition-colors duration-(--motion-duration) ease-(--motion-easing)",
          active ? "bg-control-active" : "hover:bg-control",
        )}
      >
        <span
          className="h-10 w-[60px] shrink-0 rounded-xs bg-control bg-(image:--thumb) bg-cover bg-center outline outline-1 -outline-offset-1 outline-outline-image"
          style={{ "--thumb": thumbnail ? `url(${thumbnail})` : "none" }}
        />
        <span className="flex min-w-0 grow flex-col gap-xxs">
          <span
            className={cn(
              "truncate text-s font-medium",
              active ? "text-foreground" : "text-secondary-text",
            )}
          >
            {profile.name}
          </span>
          <span className="font-data text-xs text-tertiary-text tabular-nums">
            {profile.groups.length} groups
          </span>
        </span>
        {active ? <Icon icon={Tick02Icon} size={14} className="shrink-0 text-foreground" /> : null}
      </button>
      <DeleteButton profile={profile} />
    </div>
  );
}

function apply(profile: Profile, percent: number): void {
  applyProfileTo(
    profile.id,
    profile.partial,
    percent / 100,
    `Profile ${profile.name}`,
    profile.film,
  );
}

function Empty() {
  return (
    <div className="px-s py-m text-xs text-tertiary-text">
      <p className="text-s font-medium text-secondary-text">No profiles yet</p>
      <p className="pt-xxs">
        A profile saves a look, such as tone, color and grain, to reuse on other photos.
      </p>
    </div>
  );
}

export function ProfilesSection() {
  const saved = useStore(profiles, (state) => state);
  const photoId = useStore(develop, (state) => state.photoId);
  const appliedId = useStore(develop, (state) => state.applied?.profileId);
  const [amount, setAmount] = useState(100);
  const thumbnail = photoId ? photoRecord(photoId)?.thumbnail : undefined;

  const changeAmount = (percent: number) => {
    setAmount(percent);
    const active = saved.find((profile) => profile.id === appliedId);
    if (active) {
      apply(active, percent);
    }
  };

  return (
    <section className="flex flex-col gap-xxs" aria-label="Profiles">
      <div className="flex h-8 items-center justify-between px-s">
        <h2 className="text-s font-heavy text-secondary-text">Profiles</h2>
        <span className="font-data text-xs text-tertiary-text tabular-nums">
          {saved.length} saved
        </span>
      </div>
      {saved.length === 0 ? <Empty /> : null}
      <div className="flex max-h-[300px] flex-col gap-xxs overflow-y-auto">
        {saved.map((profile) => (
          <ProfileRow
            key={profile.id}
            profile={profile}
            active={profile.id === appliedId}
            thumbnail={thumbnail}
            onApply={(chosen) => apply(chosen, amount)}
          />
        ))}
      </div>
      {saved.length > 0 ? (
        <div className="dialkit-root pt-xs" data-mode="inline" data-theme="dark">
          <Slider
            label="Amount"
            value={amount}
            min={0}
            max={200}
            step={1}
            unit="%"
            onChange={changeAmount}
          />
        </div>
      ) : null}
      <div className="pt-xs">
        <SaveProfile />
      </div>
    </section>
  );
}
