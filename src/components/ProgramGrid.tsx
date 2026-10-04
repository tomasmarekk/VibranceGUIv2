// Program library as large icon tiles, like the original's icon list.
// Pressing a tile opens its editor; the live tile is the profile applied right now.
import { Button } from "@heroui/react";
import { formatPercent } from "../format";
import type { Profile } from "../types";
import { AppIcon } from "./AppIcon";

interface ProgramGridProps {
  profiles: Profile[];
  activeId: string | null;
  iconFor: (path: string) => string | null | undefined;
  onOpen: (profile: Profile) => void;
}

/** Tiles show each program's icon, name and vibrance, or a live marker when applied. */
export function ProgramGrid({ profiles, activeId, iconFor, onOpen }: ProgramGridProps) {
  return <ul className="program-grid" aria-label="Programs">
    {profiles.map((profile) => {
      const active = profile.id === activeId;
      return <li key={profile.id}>
        <Button variant="tertiary" className={`program-tile ${active ? "is-active" : ""}`} onPress={() => onOpen(profile)}>
          <AppIcon name={profile.name} src={iconFor(profile.executablePath)} size="lg" />
          <span className="program-tile__text">
            <span className="program-tile__name">{profile.name}</span>
            <span className="program-tile__meta">{active ? <><span className="live-dot" aria-hidden="true" />Active</> : `Vibrance ${formatPercent(profile.color.vibrance)}`}</span>
          </span>
        </Button>
      </li>;
    })}
  </ul>;
}
