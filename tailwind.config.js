/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        surface: {
          0: "#0b0d10",
          1: "#12151a",
          2: "#1a1f27",
          3: "#242b36",
        },
        ink: {
          1: "#e8edf5",
          2: "#a8b3c4",
          3: "#6b778a",
        },
        accent: {
          DEFAULT: "#5b8cff",
          soft: "#243656",
        },
        ok: "#3ecf8e",
        warn: "#f0b429",
        danger: "#f07178",
      },
      fontFamily: {
        mono: ["ui-monospace", "SFMono-Regular", "Menlo", "monospace"],
      },
    },
  },
  plugins: [],
};
