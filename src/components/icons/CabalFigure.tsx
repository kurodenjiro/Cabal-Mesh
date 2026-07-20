import React from "react";

interface CabalFigureProps {
    size?: number;
    className?: string;
    eyeColor?: string;
}

/**
 * A minimal hooded/cloaked silhouette — the app's recurring "cabal member"
 * motif, used wherever the UI previously had a generic robot/person emoji.
 */
export const CabalFigure: React.FC<CabalFigureProps> = ({ size = 24, className = "", eyeColor = "currentColor" }) => (
    <svg
        width={size}
        height={size}
        viewBox="0 0 32 32"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className={className}
    >
        <path
            d="M16 2C11 5 9 10 9 14c0 1.2.2 2.2.6 3.1C6.8 19.3 5 23 5 27.5c0 .8.6 1.5 1.5 1.5h19c.9 0 1.5-.7 1.5-1.5 0-4.5-1.8-8.2-4.6-10.4.4-.9.6-1.9.6-3.1 0-4-2-9-7-12Z"
            fill="currentColor"
            fillOpacity="0.9"
        />
        <path
            d="M16 4.3c-3.6 2.7-5 6.6-5 9.7 0 3.9 2.4 6.4 5 6.4s5-2.5 5-6.4c0-3.1-1.4-7-5-9.7Z"
            fill="black"
            fillOpacity="0.35"
        />
        <circle cx="13.1" cy="14.5" r="1" fill={eyeColor} />
        <circle cx="18.9" cy="14.5" r="1" fill={eyeColor} />
    </svg>
);

export default CabalFigure;
