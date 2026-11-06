import type { BirdBrand } from "@avandar/aiviary-core";

/** How Pigeon looks: its picture, its name, and its colors. */
export const PIGEON_BRAND: BirdBrand = {
  name: "pigeon",
  displayName: "Pigeon",
  tagline: "A personal assistant for your brain: notes, tasks, and knowledge.",
  art: [
    "      .-.",
    "     (o  )>",
    "     /    \\",
    "    (  \\/  )_",
    "     \\_____/",
    "       ^ ^",
  ],
  banner: [
    "██████╗ ██╗ ██████╗ ███████╗ ██████╗ ███╗   ██╗",
    "██╔══██╗██║██╔════╝ ██╔════╝██╔═══██╗████╗  ██║",
    "██████╔╝██║██║  ███╗█████╗  ██║   ██║██╔██╗ ██║",
    "██╔═══╝ ██║██║   ██║██╔══╝  ██║   ██║██║╚██╗██║",
    "██║     ██║╚██████╔╝███████╗╚██████╔╝██║ ╚████║",
    "╚═╝     ╚═╝ ╚═════╝ ╚══════╝ ╚═════╝ ╚═╝  ╚═══╝",
  ],
  palette: {
    accent: "okhsl(150 55% 64%)",
    secondary: "okhsl(300 50% 70%)",
  },
  workingMessage: "Carrying the message...",
};
