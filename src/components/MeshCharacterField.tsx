import React, { useEffect, useRef, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Peer } from "../types";
import { SpriteCharacter } from "./icons/SpriteCharacter";
import { addressClass, CLASS_LABEL, CharacterClass } from "../lib/rpgFlavor";

interface MeshCharacterFieldProps {
    peers: Peer[];
    /** Your own real wallet address, used to pick your character sprite and
     * to label your own avatar the same way peers are labeled. */
    myAddress?: string | null;
}

/** Per-class label tint + tether glow color — the class itself is a
 * deterministic hash of the real address (see addressClass); the actual
 * visual distinction between classes now comes from each having its own
 * sprite character (see SpriteCharacter), not a colored shape. */
const CLASS_STYLE: Record<CharacterClass, { text: string; glow: string }> = {
    knight: { text: "text-nobody-primary", glow: "rgba(38,49,94,0.55)" },
    scout: { text: "text-nobody-gold", glow: "rgba(184,134,15,0.55)" },
    mystic: { text: "text-nobody-accent", glow: "rgba(156,79,110,0.55)" },
    forger: { text: "text-slate-400", glow: "rgba(148,163,184,0.55)" },
};

const peerPosition = (peer: Peer, index: number, total: number) => {
    const seed = peer.id.charCodeAt(0) % 360;
    const angle = (index * (360 / Math.max(total, 1))) + seed;
    const radius = 140 + (index % 3) * 70;
    const x = Math.cos((angle * Math.PI) / 180) * radius;
    const y = Math.sin((angle * Math.PI) / 180) * radius * 0.5 - 60;
    const distance = Math.sqrt(x * x + y * y);
    const lineAngle = (Math.atan2(y, x) * 180) / Math.PI;
    return { x, y, distance, lineAngle };
};

// Matches the roam motion's transition below — used to pause the walk-cycle
// animation (show idle) while the character is turned around at each end,
// instead of always playing the walk frames non-stop. The roam cycle walks
// for the first 40%, pauses turned around for 10%, walks back for 40%, then
// pauses again for the final 10% before looping.
const ROAM_DURATION_S = 10;
const ROAM_PAUSE_1: [number, number] = [0.4, 0.5];
const ROAM_PAUSE_2: [number, number] = [0.9, 1.0];

export const MeshCharacterField: React.FC<MeshCharacterFieldProps> = ({ peers, myAddress }) => {
    const myClass = addressClass(myAddress || "self");

    const [roamWalking, setRoamWalking] = useState(true);
    const roamStartRef = useRef(Date.now());

    useEffect(() => {
        const tick = () => {
            const elapsed = ((Date.now() - roamStartRef.current) / 1000) % ROAM_DURATION_S;
            const phase = elapsed / ROAM_DURATION_S;
            const isPaused = (phase >= ROAM_PAUSE_1[0] && phase <= ROAM_PAUSE_1[1]) || phase >= ROAM_PAUSE_2[0];
            setRoamWalking((prev) => (prev === !isPaused ? prev : !isPaused));
        };
        const id = setInterval(tick, 150);
        return () => clearInterval(id);
    }, []);

    return (
        <div className="absolute inset-0 flex items-center justify-center overflow-hidden pointer-events-none">
            {/* Perspective ground grid */}
            <div
                className="absolute left-0 right-0 bottom-[28%] h-[220px] opacity-20"
                style={{
                    backgroundImage:
                        "repeating-linear-gradient(90deg, rgba(184,134,15,0.5) 0px, rgba(184,134,15,0.5) 1px, transparent 1px, transparent 40px)," +
                        "repeating-linear-gradient(0deg, rgba(184,134,15,0.5) 0px, rgba(184,134,15,0.5) 1px, transparent 1px, transparent 40px)",
                    transform: "perspective(400px) rotateX(60deg)",
                    maskImage: "linear-gradient(to top, black, transparent)",
                }}
            />

            {/* Ground line / mesh horizon */}
            <div className="absolute left-[10%] right-[10%] bottom-[28%] h-px bg-nobody-gold/40" style={{ boxShadow: "0 0 12px rgba(184,134,15,0.4)" }} />

            {/* Central beacon (mesh hub) */}
            <motion.div
                className="absolute bottom-[28%] left-1/2 -translate-x-1/2 translate-y-1/2 z-10"
                animate={{ opacity: [0.6, 1, 0.6] }}
                transition={{ duration: 2.5, repeat: Infinity, ease: "easeInOut" }}
            >
                <div className="w-2 h-2 bg-nobody-gold pixel-corners-sm" style={{ boxShadow: "0 0 16px rgba(184,134,15,0.7)" }} />
            </motion.div>

            {/* Tether lines from beacon to each peer */}
            {peers.map((peer, index) => {
                const { distance, lineAngle } = peerPosition(peer, index, peers.length);
                const color = CLASS_STYLE[addressClass(peer.id)];
                return (
                    <motion.div
                        key={`tether-${peer.id}`}
                        className="absolute left-1/2 bottom-[28%] h-px origin-left"
                        style={{
                            width: distance,
                            transform: `rotate(${lineAngle}deg)`,
                            backgroundImage: `repeating-linear-gradient(to right, ${color.glow} 0 4px, transparent 4px 8px)`,
                        }}
                        initial={{ opacity: 0, scaleX: 0 }}
                        animate={{ opacity: 0.6, scaleX: 1 }}
                        transition={{ duration: 0.8 }}
                    />
                );
            })}

            {/* Peer nodes */}
            <AnimatePresence>
                {peers.map((peer, index) => {
                    const { x, y } = peerPosition(peer, index, peers.length);
                    const cls = addressClass(peer.id);
                    const color = CLASS_STYLE[cls];

                    return (
                        <motion.div
                            key={peer.id}
                            className="absolute left-1/2 bottom-[24%] z-10"
                            initial={{ scale: 0, opacity: 0, x, y }}
                            animate={{ scale: 1, opacity: 1, x, y }}
                            exit={{ scale: 0, opacity: 0 }}
                            transition={{ type: "spring", stiffness: 120 }}
                        >
                            <div className="relative -translate-x-1/2 flex flex-col items-center gap-0.5">
                                <div className={`text-[11px] font-mono font-bold whitespace-nowrap bg-nobody-charcoal/80 px-1.5 py-0.5 pixel-corners-sm ${peer.walletAddress ? "normal-case" : "uppercase"} ${color.text}`}>
                                    {peer.walletAddress
                                        ? `${peer.walletAddress.slice(0, 6)}...${peer.walletAddress.slice(-4)}`
                                        : CLASS_LABEL[cls]}
                                </div>
                                <SpriteCharacter characterClass={cls} height={38} walking />
                            </div>
                        </motion.div>
                    );
                })}
            </AnimatePresence>

            {/* Your own roaming character — walks right, turns around, walks left */}
            <motion.div
                className="absolute bottom-[24%] z-20"
                animate={{ x: [-180, 180, 180, -180, -180] }}
                transition={{
                    duration: ROAM_DURATION_S,
                    repeat: Infinity,
                    ease: "easeInOut",
                    times: [0, ROAM_PAUSE_1[0], ROAM_PAUSE_1[1], ROAM_PAUSE_2[0], 1],
                }}
            >
                <div className="flex flex-col items-center gap-0.5">
                    {myAddress && (
                        <div className={`text-[11px] font-mono font-bold whitespace-nowrap normal-case bg-nobody-charcoal/80 px-1.5 py-0.5 pixel-corners-sm ${CLASS_STYLE[myClass].text}`}>
                            {myAddress.slice(0, 6)}...{myAddress.slice(-4)}
                        </div>
                    )}
                    <motion.div
                        animate={{ scaleX: [1, 1, -1, -1, 1, 1] }}
                        transition={{ duration: ROAM_DURATION_S, repeat: Infinity, times: [0, 0.45, 0.45, 0.95, 0.95, 1] }}
                    >
                        <SpriteCharacter characterClass={myClass} height={56} walking={roamWalking} />
                    </motion.div>
                </div>
            </motion.div>
        </div>
    );
};

export default MeshCharacterField;
