// Mounts the local HeroUI interface inside the desktop webview.
// Strict Mode keeps subscription cleanup exercised during development.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/inter/opsz.css";
import App from "./App";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("The application root is missing.");
createRoot(root).render(<StrictMode><App /></StrictMode>);
