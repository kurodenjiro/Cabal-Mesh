import React, { useEffect, useState } from "react";
import { CharacterClass } from "../../lib/rpgFlavor";

// Side-view chibi character frames, cropped from the SuperRetroWorld
// Character Pack (RPG Maker MV format) the user supplied — not MapleStory's
// own (copyrighted) art. Note: SuperRetroWorld's license disallows NFT/AI
// projects; used here at the user's explicit direction, accepting that risk.
import soldierWalk1 from "../../assets/sprites/soldier_walk1.png";
import soldierWalk2 from "../../assets/sprites/soldier_walk2.png";
import soldierIdle from "../../assets/sprites/soldier_idle.png";
import adventurerWalk1 from "../../assets/sprites/adventurer_walk1.png";
import adventurerWalk2 from "../../assets/sprites/adventurer_walk2.png";
import adventurerIdle from "../../assets/sprites/adventurer_idle.png";
import femaleWalk1 from "../../assets/sprites/female_walk1.png";
import femaleWalk2 from "../../assets/sprites/female_walk2.png";
import femaleIdle from "../../assets/sprites/female_idle.png";
import playerWalk1 from "../../assets/sprites/player_walk1.png";
import playerWalk2 from "../../assets/sprites/player_walk2.png";
import playerIdle from "../../assets/sprites/player_idle.png";

const FRAMES: Record<CharacterClass, { walk1: string; walk2: string; idle: string }> = {
    knight: { walk1: soldierWalk1, walk2: soldierWalk2, idle: soldierIdle },
    scout: { walk1: adventurerWalk1, walk2: adventurerWalk2, idle: adventurerIdle },
    mystic: { walk1: femaleWalk1, walk2: femaleWalk2, idle: femaleIdle },
    forger: { walk1: playerWalk1, walk2: playerWalk2, idle: playerIdle },
};

interface SpriteCharacterProps {
    characterClass: CharacterClass;
    walking?: boolean;
    height?: number;
    facing?: "left" | "right";
    className?: string;
}

/** A small side-view character sprite that alternates between its two walk
 * frames while `walking`, or holds its idle pose otherwise. */
export const SpriteCharacter: React.FC<SpriteCharacterProps> = ({ characterClass, walking = true, height = 48, facing = "right", className = "" }) => {
    const [frame, setFrame] = useState(0);
    const sprites = FRAMES[characterClass];

    useEffect(() => {
        if (!walking) return;
        const interval = setInterval(() => setFrame((f) => (f === 0 ? 1 : 0)), 220);
        return () => clearInterval(interval);
    }, [walking]);

    const src = !walking ? sprites.idle : frame === 0 ? sprites.walk1 : sprites.walk2;

    return (
        <img
            src={src}
            alt={characterClass}
            className={className}
            draggable={false}
            style={{
                height,
                width: "auto",
                imageRendering: "pixelated",
                transform: facing === "left" ? "scaleX(-1)" : undefined,
                filter: "drop-shadow(0 2px 4px rgba(0,0,0,0.35))",
            }}
        />
    );
};

export default SpriteCharacter;
