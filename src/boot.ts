/** Injected by Rust via `initialization_script` before the page loads. */
export type Boot =
  | { view: "selector"; monitor: number; imagePath: string; width: number; height: number }
  | { view: "result"; imagePath: string; width: number; height: number }
  | { view: "settings" }
  | { view: "history" };

declare global {
  interface Window {
    __OVERTEXT__?: Boot;
  }
}
