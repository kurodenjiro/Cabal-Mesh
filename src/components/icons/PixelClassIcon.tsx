import React from "react";
import { SpriteCharacter } from "./SpriteCharacter";
import { CharacterClass, addressClass } from "../../lib/rpgFlavor";

interface PixelClassIconProps {
    /** Either pass a real address to derive the class deterministically, or
     * pass an explicit class directly. */
    address?: string;
    characterClass?: CharacterClass;
    size?: number;
    className?: string;
}

/** Small static (idle-pose) character sprite — same class-per-address
 * mapping used in MeshCharacterField, for a consistent identity cue
 * anywhere a counterparty's address shows up. */
export const PixelClassIcon: React.FC<PixelClassIconProps> = ({ address, characterClass, size = 20, className = "" }) => {
    const cls = characterClass ?? (address ? addressClass(address) : "knight");
    return <SpriteCharacter characterClass={cls} height={size} walking={false} className={className} />;
};

export default PixelClassIcon;
