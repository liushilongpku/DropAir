import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// Apply the stored/system theme before the first paint to avoid a light flash.
const storedTheme = localStorage.getItem("dropair.theme");
const prefersDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
document.documentElement.dataset.theme =
  storedTheme === "light" || storedTheme === "dark" ? storedTheme : prefersDark ? "dark" : "light";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
