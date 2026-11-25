import type { Config } from 'tailwindcss';

const config: Config = {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        ink: {
          950: '#060a0f',
          925: '#080d13',
          900: '#0b1017',
          875: '#0e141c',
          850: '#101821',
          800: '#141d28',
          750: '#192330',
          700: '#1f2b3a',
          650: '#27364a',
        },
        accent: {
          DEFAULT: '#34d399',
          soft: '#6ee7b7',
          dim: '#10b981',
          deep: '#064e3b',
        },
        signal: {
          strong: '#34d399',
          good: '#a3e635',
          fair: '#fbbf24',
          weak: '#fb923c',
          none: '#64748b',
        },
        role: {
          client: '#34d399',
          repeater: '#fbbf24',
          tracker: '#38bdf8',
          gateway: '#a78bfa',
          room: '#f472b6',
        },
      },
      fontFamily: {
        sans: [
          'Inter Variable',
          'Inter',
          'ui-sans-serif',
          'system-ui',
          '-apple-system',
          'Segoe UI',
          'Roboto',
          'sans-serif',
        ],
        mono: ['ui-monospace', 'SFMono-Regular', 'Menlo', 'Consolas', 'monospace'],
      },
      fontSize: {
        '2xs': ['0.6875rem', { lineHeight: '1rem' }],
        '3xs': ['0.625rem', { lineHeight: '0.875rem' }],
      },
      boxShadow: {
        panel: '0 1px 0 0 rgba(255,255,255,0.03) inset, 0 12px 32px -12px rgba(0,0,0,0.6)',
        pop: '0 20px 50px -20px rgba(0,0,0,0.75), 0 0 0 1px rgba(148,163,184,0.12)',
        glow: '0 0 0 1px rgba(52,211,153,0.25), 0 0 24px -6px rgba(52,211,153,0.45)',
      },
      borderRadius: {
        xl2: '1rem',
        xl3: '1.25rem',
      },
      transitionTimingFunction: {
        smooth: 'cubic-bezier(0.22, 1, 0.36, 1)',
      },
      keyframes: {
        'fade-in': {
          from: { opacity: '0' },
          to: { opacity: '1' },
        },
        'slide-up': {
          from: { opacity: '0', transform: 'translateY(6px)' },
          to: { opacity: '1', transform: 'translateY(0)' },
        },
        'slide-in-right': {
          from: { opacity: '0', transform: 'translateX(16px)' },
          to: { opacity: '1', transform: 'translateX(0)' },
        },
        'pulse-soft': {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0.35' },
        },
        'ping-slow': {
          '0%': { transform: 'scale(0.7)', opacity: '0.7' },
          '80%, 100%': { transform: 'scale(2.2)', opacity: '0' },
        },
        shimmer: {
          '100%': { transform: 'translateX(100%)' },
        },
        'bounce-dot': {
          '0%, 60%, 100%': { transform: 'translateY(0)' },
          '30%': { transform: 'translateY(-3px)' },
        },
      },
      animation: {
        'fade-in': 'fade-in 160ms ease-out both',
        'slide-up': 'slide-up 180ms cubic-bezier(0.22, 1, 0.36, 1) both',
        'slide-in-right': 'slide-in-right 220ms cubic-bezier(0.22, 1, 0.36, 1) both',
        'pulse-soft': 'pulse-soft 2.4s ease-in-out infinite',
        'ping-slow': 'ping-slow 2.6s cubic-bezier(0, 0, 0.2, 1) infinite',
        shimmer: 'shimmer 1.8s infinite',
        'bounce-dot': 'bounce-dot 1.2s ease-in-out infinite',
      },
    },
  },
  plugins: [],
};

export default config;
