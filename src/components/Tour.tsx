// The tour: six of SSHeesh's best tricks, a minute in all, each acted out by
// a little picture on a made-up server. You start it yourself (from the end
// of the hello, or "Take the tour" in the command bar), and can skip it at
// any point.

import { RotateCcw } from "lucide-react";
import { useEffect, useState, type ComponentType } from "react";
import { cn } from "../lib/format";
import { shortcutLabel } from "../lib/keys";
import { toggleStreamer } from "../lib/privacy";
import { useTour } from "../lib/tour";
import { useSettings } from "../lib/settings";
import { Modal } from "./Overlays";
import { SceneCommandBar, SceneDragSplit, SceneDurable, ScenePeek, SceneStreamer, SceneTabs } from "./TourScenes";
import { Button, Kbd } from "./ui";


interface Step {
  scene: ComponentType;
  title: string;
  body: () => string;
  /** For pros: one quieter line. */
  detail?: () => string;
  keys?: () => string;
}

const STEPS: Step[] = [
  {
    scene: SceneCommandBar,
    title: "Find anything",
    body: () => "Servers, folders, commands and settings, from one bar.",
    detail: () => "Commands say which server they run on before they run.",
    keys: () => shortcutLabel("palette"),
  },
  {
    scene: SceneTabs,
    title: "Every server keeps its tabs",
    body: () =>
      useSettings.getState().tabsPlacement === "top"
        ? "Each server has its own tabs along the top. The list says when an agent on any server needs you."
        : "Tabs live under their server. Agents on any server show up there too.",
    detail: () => "“Needs you” means an agent is waiting for an answer.",
  },
  {
    scene: SceneDurable,
    title: "Lost Wi-Fi? Work keeps running",
    body: () => "Your session stays alive on the server and picks up where it was.",
    detail: () => "Uses tmux on the server.",
  },
  {
    scene: ScenePeek,
    title: "Peek at a file",
    body: () => "Click a path in the output to see the file beside the terminal.",
    detail: () => "Markdown shows formatted. Links open in your browser.",
  },
  {
    scene: SceneDragSplit,
    title: "Drag to split",
    body: () => "Drop a tab onto the page to see two at once. Drag it back to the list to undo.",
    keys: () => shortcutLabel("toggleSplit"),
  },
  {
    scene: SceneStreamer,
    title: "Streaming? Hide it all",
    body: () => "One switch hides server names, addresses and users everywhere, before anyone sees them.",
    detail: () => "No account, no cloud: your servers and keys stay on this computer.",
    keys: () => shortcutLabel("streamer"),
  },
];

export function Tour() {
  const open = useTour((s) => s.open);
  const close = useTour((s) => s.close);
  const streamer = useSettings((s) => s.streamerMode);
  // It's only on screen while open, so it starts where it was asked to.
  const [step, setStep] = useState(() => useTour.getState().start);
  // Bumping it plays the scene again.
  const [take, setTake] = useState(0);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowRight") setStep((x) => Math.min(STEPS.length - 1, x + 1));
      else if (e.key === "ArrowLeft") setStep((x) => Math.max(0, x - 1));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const s = STEPS[step];
  const Scene = s.scene;
  const last = step === STEPS.length - 1;
  return (
    <Modal open={open} onClose={close} width={640}>
      <div className="flex flex-col gap-4 p-6">
        {/* Where you are: one segment per step, the ones behind you filled. */}
        <div className="flex gap-1" role="tablist" aria-label="Tour steps">
          {STEPS.map((x, i) => (
            <button
              key={x.title}
              type="button"
              role="tab"
              aria-selected={i === step}
              aria-label={`Step ${i + 1}: ${x.title}`}
              onClick={() => setStep(i)}
              className="group h-4 flex-1 py-1.5"
            >
              <span className={cn("block h-1 rounded-full transition-colors duration-300", i <= step ? "bg-accent" : "bg-ink/[0.12] group-hover:bg-ink/[0.2]")} />
            </button>
          ))}
        </div>
        <div key={step} className="animate-fade-in flex flex-col gap-4">
          <div className="relative">
            <Scene key={take} />
            <button
              type="button"
              onClick={() => setTake((t) => t + 1)}
              className="absolute right-2 bottom-2 flex h-6 items-center gap-1 rounded-[6px] bg-raised/90 px-2 text-[11px] text-fg-2 ring-1 ring-ink/[0.1] hover:text-fg"
            >
              <RotateCcw size={11} /> Replay
            </button>
          </div>
          <div className="min-h-[96px]">
            <h2 className="flex items-center gap-2.5 text-[19px] font-semibold tracking-tight text-fg">
              {s.title}
              {s.keys && <Kbd>{s.keys()}</Kbd>}
            </h2>
            <p className="mt-1.5 max-w-[540px] text-[13.5px] leading-relaxed text-fg-2">{s.body()}</p>
            {s.detail && <p className="mt-1 text-[12px] text-fg-3">{s.detail()}</p>}
          </div>
        </div>
        <div className="flex items-center gap-3">
          <span className="text-[11.5px] text-fg-3 tabular-nums">
            {step + 1} of {STEPS.length}
          </span>
          <span className="flex-1" />
          {last ? (
            <Button variant="ghost" onClick={toggleStreamer}>
              {streamer ? "Streamer mode is on" : "Turn it on now"}
            </Button>
          ) : (
            <Button variant="ghost" onClick={close}>
              Skip tour
            </Button>
          )}
          {step > 0 && (
            <Button variant="ghost" onClick={() => setStep((x) => x - 1)}>
              Back
            </Button>
          )}
          <Button variant="primary" autoFocus onClick={() => (last ? close() : setStep((x) => x + 1))}>
            {last ? "Done" : "Next"}
          </Button>
        </div>
        {last && <p className="-mt-1 text-right text-[11.5px] text-fg-3">The tour is in the command bar any time: {shortcutLabel("palette")}, then “tour”.</p>}
      </div>
    </Modal>
  );
}
