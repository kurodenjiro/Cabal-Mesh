/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      fontFamily: {
        sans: ['"Inter"', 'ui-sans-serif', '-apple-system', 'BlinkMacSystemFont', '"Segoe UI"', 'Roboto', 'sans-serif'],
        mono: ['"JetBrains Mono"', 'ui-monospace', 'SFMono-Regular', 'Menlo', 'monospace'],
        serif: ['"Cormorant Garamond"', '"Iowan Old Style"', 'Georgia', 'serif'],
      },
      colors: {
        // Neutral scale reskinned for the dark occult theme — every
        // component consistently uses `slate-*` for text/surfaces/borders,
        // so overriding the scale here flips the whole app in one place.
        slate: {
          50: '#161022',
          100: '#1c1530',
          200: '#2a213e',
          300: '#3d2f57',
          400: '#8b7fa8',
          500: '#a394c4',
          600: '#c3b6e0',
          700: '#dcd2f0',
          800: '#ece5f7',
          900: '#f5f1fc',
        },
        // Primary accent — eldritch emerald (glows on dark)
        'nobody-mint': '#34d399',
        'nobody-mint-soft': '#064e3b',
        // Secondary accent — occult violet
        'nobody-violet': '#a78bfa',
        'nobody-violet-soft': '#3b1f6b',
        // Surfaces
        'nobody-charcoal': '#171029',
        'nobody-dark': '#0a0714',
        'nobody-ink': '#f5f1fc',
      },
      boxShadow: {
        card: '0 0 0 1px rgba(167, 139, 250, 0.08), 0 2px 12px -2px rgba(0, 0, 0, 0.5)',
        'card-lg': '0 0 0 1px rgba(167, 139, 250, 0.12), 0 12px 32px -6px rgba(0, 0, 0, 0.6), 0 0 40px -10px rgba(167, 139, 250, 0.15)',
        glow: '0 0 24px -4px rgba(52, 211, 153, 0.35)',
      },
      animation: {
        'pulse-slow': 'pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite',
        'radar-ping': 'ping 2s cubic-bezier(0, 0, 0.2, 1) infinite',
        flicker: 'flicker 4s ease-in-out infinite',
        drift: 'drift 12s ease-in-out infinite',
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
      },
    },
  },
  plugins: [],
}
