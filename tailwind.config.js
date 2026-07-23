/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      fontFamily: {
        sans: ['"JetBrains Mono"', 'ui-monospace', 'SFMono-Regular', 'Menlo', 'monospace'],
        mono: ['"JetBrains Mono"', 'ui-monospace', 'SFMono-Regular', 'Menlo', 'monospace'],
        pixel: ['"Press Start 2P"', '"JetBrains Mono"', 'ui-monospace', 'monospace'],
      },
      colors: {
        // Neutral scale reskinned for the dark ruins/night theme — every
        // component consistently uses `slate-*` for text/surfaces/borders,
        // so overriding the scale here flips the whole app in one place.
        // Direction is inverted from the old light-parchment scale: low
        // numbers are now dark (surfaces), high numbers are light (text).
        slate: {
          50: '#1c2118',
          100: '#242a1e',
          200: '#3a4030',
          300: '#525a3f',
          400: '#9a9c6e',
          500: '#b3b47f',
          600: '#c7c797',
          700: '#d7d6ac',
          800: '#e6e4c6',
          900: '#f2f0dd',
        },
        // Primary accent — deep indigo/navy (dominant, ~70% of accent use)
        'nobody-primary': '#7d8fd6',
        'nobody-primary-soft': 'rgba(125, 143, 214, 0.15)',
        // Secondary accent — halo gold (used for highlights/rays, ~25%)
        'nobody-gold': '#c9a94f',
        'nobody-gold-soft': 'rgba(201, 169, 79, 0.15)',
        // Rare tertiary accent — dusty rose, used sparingly for special/shark states
        'nobody-accent': '#c97fa0',
        'nobody-accent-soft': 'rgba(201, 127, 160, 0.15)',
        // Surfaces
        'nobody-charcoal': '#12160f',
        'nobody-dark': '#0a0d08',
        'nobody-ink': '#12160f',
      },
      boxShadow: {
        card: '0 1px 2px 0 rgba(38, 49, 94, 0.06), 0 2px 8px -2px rgba(38, 49, 94, 0.08)',
        'card-lg': '0 8px 24px -6px rgba(38, 49, 94, 0.16), 0 2px 6px -2px rgba(38, 49, 94, 0.1)',
        glow: '0 0 0 1px rgba(38, 49, 94, 0.15), 0 4px 20px -4px rgba(38, 49, 94, 0.3)',
        'glow-gold': '0 0 0 1px rgba(184, 134, 15, 0.25), 0 4px 20px -4px rgba(184, 134, 15, 0.4)',
        'glow-accent': '0 0 0 1px rgba(156, 79, 110, 0.25), 0 4px 20px -4px rgba(156, 79, 110, 0.4)',
      },
      animation: {
        'pulse-slow': 'pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite',
        'radar-ping': 'ping 2s cubic-bezier(0, 0, 0.2, 1) infinite',
        flicker: 'flicker 4s ease-in-out infinite',
        drift: 'drift 12s ease-in-out infinite',
        scanline: 'scanline 8s linear infinite',
        walk: 'walk 0.6s steps(2) infinite',
        'spin-slow': 'spin 24s linear infinite',
        rise: 'rise 6s ease-in infinite',
        glitch: 'glitch 3.2s ease-in-out infinite',
      },
      keyframes: {
        flicker: {
          '0%, 100%': { opacity: 1 },
          '45%': { opacity: 0.85 },
          '50%': { opacity: 0.55 },
          '55%': { opacity: 0.85 },
        },
        drift: {
          '0%, 100%': { transform: 'translate(0, 0)' },
          '50%': { transform: 'translate(6px, -10px)' },
        },
        scanline: {
          '0%': { transform: 'translateY(-100%)' },
          '100%': { transform: 'translateY(100%)' },
        },
        walk: {
          '0%, 100%': { transform: 'translateY(0)' },
          '50%': { transform: 'translateY(-2px)' },
        },
        rise: {
          '0%': { transform: 'translateY(0) scale(1)', opacity: 0 },
          '10%': { opacity: 0.7 },
          '100%': { transform: 'translateY(-140px) scale(0.4)', opacity: 0 },
        },
        glitch: {
          '0%, 92%, 100%': { transform: 'translate(0, 0)', opacity: 1 },
          '93%': { transform: 'translate(-2px, 1px)', opacity: 0.7 },
          '94%': { transform: 'translate(2px, -1px)', opacity: 1 },
          '95%': { transform: 'translate(-1px, 0)', opacity: 0.8 },
          '96%': { transform: 'translate(0, 0)', opacity: 1 },
        },
      },
    },
  },
  plugins: [],
}
