<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open, save } from "@tauri-apps/plugin-dialog";

  type ElementKind = "resistor" | "capacitor" | "inductor" | "voltage_source" | "current_source" | "diode" | "ground";
  type Terminal = "a" | "b";
  type Point = { x: number; y: number };
  type Pin = { componentId: string; terminal: Terminal };
  type SchematicElement = { id: string; kind: ElementKind; label: string; value: number; x: number; y: number };
  type Wire = { from: Pin; to: Pin };
  type DcResult = { nodeVoltages: { label: string; voltage: number }[]; deviceCurrents: { label: string; current: number }[] };
  type TransientResult = { times: number[]; traces: { label: string; values: number[] }[] };
  type ResourceSnapshot = { totalMemoryMb: number | null; availableMemoryMb: number | null; usedMemoryMb: number | null; memoryPressure: string; logicalCpuCores: number; cpuUsagePercent: number | null };
  type SimulationEstimate = { unknownCount: number; timePoints: number; estimatedPeakMb: number; recommendedBudgetMb: number | null; fitsRecommendedBudget: boolean | null; recommendedWorkers: number };
  type ProjectResult = { name: string; circuit: { elements: SchematicElement[]; wires: Wire[] }; simulationConfig: { tStop: number; tStep: number } | null; results: { dcResult?: DcResult | null; transientResult?: TransientResult | null } | null; recoveredFrom: string };

  const canvasWidth = 900;
  const canvasHeight = 610;
  const defaults: Record<ElementKind, number> = {
    resistor: 1000,
    capacitor: 1e-6,
    inductor: 0.01,
    voltage_source: 5,
    current_source: 0.001,
    diode: 2.52e-9,
    ground: 0,
  };
  const barePrefixes: Record<string, number> = {
    p: 1e-12, n: 1e-9, u: 1e-6, "µ": 1e-6, m: 1e-3,
    k: 1e3, K: 1e3, M: 1e6, meg: 1e6, G: 1e9,
  };
  const componentUnits: Record<Exclude<ElementKind, "ground">, Record<string, number>> = {
    resistor: { ohm: 1, Ohm: 1, "Ω": 1, R: 1, mOhm: 1e-3, "mΩ": 1e-3, kOhm: 1e3, kohm: 1e3, "kΩ": 1e3, MOhm: 1e6, Mohm: 1e6, "MΩ": 1e6 },
    capacitor: { F: 1, mF: 1e-3, uF: 1e-6, "µF": 1e-6, nF: 1e-9, pF: 1e-12 },
    inductor: { H: 1, mH: 1e-3, uH: 1e-6, "µH": 1e-6, nH: 1e-9 },
    voltage_source: { V: 1, mV: 1e-3, uV: 1e-6, "µV": 1e-6, kV: 1e3 },
    current_source: { A: 1, mA: 1e-3, uA: 1e-6, "µA": 1e-6, nA: 1e-9, pA: 1e-12, kA: 1e3 },
    diode: { A: 1, mA: 1e-3, uA: 1e-6, "µA": 1e-6, nA: 1e-9, pA: 1e-12, kA: 1e3 },
  };

  let elements: SchematicElement[] = [];
  let wires: Wire[] = [];
  let selectedId: string | null = null;
  let wireMode = false;
  let wireStart: Pin | null = null;
  let drag: { id: string; dx: number; dy: number } | null = null;
  let idCounter = 1;
  let projectName = "Untitled Project";
  let projectPath: string | null = null;
  let recentProjects: { name: string; path: string }[] = [];
  let isDirty = false;
  let isRunning = false;
  let errorMessage = "";
  let statusMessage = "Ready";
  let dcResult: DcResult | null = null;
  let transientResult: TransientResult | null = null;
  let activeTrace = 0;
  let resource: ResourceSnapshot | null = null;
  let valueEditor = "";
  let timeStop = "0.01";
  let timeStep = "0.00001";
  let showRecent = false;
  let webgl2Available = false;
  let svgElement!: SVGSVGElement;
  let selectedElement: SchematicElement | null = null;
  let selectedWaveform: { label: string; values: number[] } | null = null;
  let waveformPath = "";
  let waveformRange = { min: 0, max: 1 };

  $: selectedElement = elements.find((element) => element.id === selectedId) ?? null;
  $: selectedWaveform = transientResult?.traces[activeTrace] ?? null;
  $: waveformPath = selectedWaveform && transientResult
    ? makeWaveformPath(transientResult.times, selectedWaveform.values)
    : "";
  $: waveformRange = selectedWaveform ? rangeOf(selectedWaveform.values) : { min: 0, max: 1 };

  function nextId(): string {
    return `part-${idCounter++}`;
  }

  function kindPrefix(kind: ElementKind): string {
    return ({ resistor: "R", capacitor: "C", inductor: "L", voltage_source: "V", current_source: "I", diode: "D", ground: "GND" } as Record<ElementKind, string>)[kind];
  }

  function addElement(kind: ElementKind) {
    const count = elements.filter((element) => element.kind === kind).length + 1;
    const id = nextId();
    const element: SchematicElement = {
      id,
      kind,
      label: kind === "ground" ? `GND${count}` : `${kindPrefix(kind)}${count}`,
      value: defaults[kind],
      x: Math.min(260 + elements.length * 36, 780),
      y: Math.min(150 + (elements.length % 5) * 58, 520),
    };
    elements = [...elements, element];
    selectedId = id;
    wireMode = false;
    wireStart = null;
    valueEditor = String(element.value);
    markDirty();
  }

  function newProject() {
    elements = [];
    wires = [];
    selectedId = null;
    wireMode = false;
    wireStart = null;
    projectName = "Untitled Project";
    projectPath = null;
    dcResult = null;
    transientResult = null;
    isDirty = false;
    errorMessage = "";
    statusMessage = "New project";
  }

  function handleKeydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      saveProject();
    }
    if (event.key === "Escape" && wireMode) {
      wireMode = false;
      wireStart = null;
      statusMessage = "Wire tool cancelled";
    }
    if ((event.key === "Delete" || event.key === "Backspace") && selectedId) {
      const target = event.target as HTMLElement;
      if (target.tagName !== "INPUT" && target.tagName !== "TEXTAREA") removeSelected();
    }
  }

  function loadDividerExample() {
    elements = [
      { id: "v1", kind: "voltage_source", label: "V1", value: 10, x: 150, y: 180 },
      { id: "r1", kind: "resistor", label: "R1", value: 1000, x: 370, y: 180 },
      { id: "r2", kind: "resistor", label: "R2", value: 1000, x: 590, y: 360 },
      { id: "g1", kind: "ground", label: "GND", value: 0, x: 150, y: 420 },
    ];
    wires = [
      { from: pin("v1", "a"), to: pin("r1", "a") },
      { from: pin("r1", "b"), to: pin("r2", "a") },
      { from: pin("v1", "b"), to: pin("g1", "a") },
      { from: pin("r2", "b"), to: pin("g1", "a") },
    ];
    idCounter = 5;
    selectedId = "r1";
    projectName = "Voltage Divider";
    dcResult = null;
    transientResult = null;
    markDirty();
    statusMessage = "Loaded voltage-divider example · 10 V input · 1 kΩ / 1 kΩ";
  }

  function pin(componentId: string, terminal: Terminal): Pin {
    return { componentId, terminal };
  }

  function terminalPosition(element: SchematicElement, terminal: Terminal): Point {
    if (element.kind === "ground") return { x: element.x, y: element.y - 16 };
    return { x: element.x + (terminal === "a" ? -48 : 48), y: element.y };
  }

  function getElement(id: string): SchematicElement | undefined {
    return elements.find((element) => element.id === id);
  }

  function wireD(wire: Wire): string {
    const fromElement = getElement(wire.from.componentId);
    const toElement = getElement(wire.to.componentId);
    if (!fromElement || !toElement) return "";
    const start = terminalPosition(fromElement, wire.from.terminal);
    const end = terminalPosition(toElement, wire.to.terminal);
    const midX = (start.x + end.x) / 2;
    return `M ${start.x} ${start.y} L ${midX} ${start.y} L ${midX} ${end.y} L ${end.x} ${end.y}`;
  }

  function clickTerminal(id: string, terminal: Terminal, event: MouseEvent) {
    event.stopPropagation();
    const target = pin(id, terminal);
    selectedId = id;
    const selected = getElement(id);
    valueEditor = selected ? String(selected.value) : "";
    if (!wireMode) return;
    if (!wireStart) {
      wireStart = target;
      statusMessage = `Choose the second terminal for ${getElement(id)?.label ?? id}`;
      return;
    }
    if (wireStart.componentId === target.componentId && wireStart.terminal === target.terminal) {
      wireStart = null;
      statusMessage = "Wire cancelled";
      return;
    }
    const alreadyConnected = wires.some((wire) =>
      (samePin(wire.from, wireStart!) && samePin(wire.to, target))
      || (samePin(wire.to, wireStart!) && samePin(wire.from, target))
    );
    if (!alreadyConnected) wires = [...wires, { from: wireStart, to: target }];
    wireStart = null;
    markDirty();
    statusMessage = "Wire connected";
  }

  function samePin(a: Pin, b: Pin): boolean {
    return a.componentId === b.componentId && a.terminal === b.terminal;
  }

  function toggleWireMode() {
    wireMode = !wireMode;
    wireStart = null;
    statusMessage = wireMode ? "Wire tool active · click two terminals" : "Selection tool active";
  }

  function beginDrag(event: PointerEvent, id: string) {
    if (wireMode || (event.target as HTMLElement).closest(".terminal-hit")) return;
    event.stopPropagation();
    selectedId = id;
    const element = getElement(id);
    const point = svgPoint(event.clientX, event.clientY);
    if (!element || !point) return;
    drag = { id, dx: element.x - point.x, dy: element.y - point.y };
    valueEditor = String(element.value);
  }

  function svgPoint(clientX: number, clientY: number): Point | null {
    if (!svgElement) return null;
    const bounds = svgElement.getBoundingClientRect();
    if (!bounds.width || !bounds.height) return null;
    return {
      x: ((clientX - bounds.left) / bounds.width) * canvasWidth,
      y: ((clientY - bounds.top) / bounds.height) * canvasHeight,
    };
  }

  function moveDrag(event: PointerEvent) {
    if (!drag) return;
    const point = svgPoint(event.clientX, event.clientY);
    if (!point) return;
    const x = Math.max(55, Math.min(canvasWidth - 55, point.x + drag.dx));
    const y = Math.max(45, Math.min(canvasHeight - 45, point.y + drag.dy));
    elements = elements.map((element) => element.id === drag!.id
      ? { ...element, x: Math.round(x / 10) * 10, y: Math.round(y / 10) * 10 }
      : element);
    isDirty = true;
  }

  function endDrag() {
    if (drag) markDirty();
    drag = null;
  }

  function clearSelection(event: MouseEvent) {
    if (event.target === svgElement) selectedId = null;
  }

  function removeSelected() {
    if (!selectedId) return;
    const removeId = selectedId;
    elements = elements.filter((element) => element.id !== removeId);
    wires = wires.filter((wire) => wire.from.componentId !== removeId && wire.to.componentId !== removeId);
    selectedId = null;
    markDirty();
  }

  function parseEngineering(text: string, kind: ElementKind): number | null {
    if (kind === "ground") return null;
    const match = text.trim().match(/^([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?)\s*(.*)$/i);
    if (!match) return null;
    const number = Number(match[1]);
    if (!Number.isFinite(number)) return null;
    const suffix = match[2].replace(/\s+/g, "");
    let multiplier = 1;
    if (suffix === "") {
      multiplier = 1;
    } else if (Object.prototype.hasOwnProperty.call(barePrefixes, suffix)) {
      multiplier = barePrefixes[suffix];
    } else if (Object.prototype.hasOwnProperty.call(componentUnits[kind], suffix)) {
      multiplier = componentUnits[kind][suffix];
    } else {
      // Explicit units are dimension-checked against the selected component.
      return null;
    }
    const result = number * multiplier;
    return Number.isFinite(result) ? result : null;
  }

  function updateSelectedLabel(value: string) {
    if (!selectedId) return;
    elements = elements.map((element) => element.id === selectedId ? { ...element, label: value } : element);
    markDirty();
  }

  function commitSelectedValue() {
    if (!selectedId) return;
    const value = parseEngineering(valueEditor, selectedElement?.kind ?? "ground");
    if (value === null) {
      errorMessage = "Value format not recognized. Examples: 4.7k, 100nF, 5V.";
      return;
    }
    elements = elements.map((element) => element.id === selectedId ? { ...element, value } : element);
    errorMessage = "";
    markDirty();
  }

  function markDirty() {
    isDirty = true;
    statusMessage = "Unsaved changes";
  }

  function circuitPayload() {
    return { elements, wires };
  }

  async function runDc() {
    if (isRunning) return;
    isRunning = true;
    errorMessage = "";
    transientResult = null;
    statusMessage = "Estimating resources…";
    try {
      const estimate = await invoke<SimulationEstimate>("estimate_simulation", { input: circuitPayload(), timePoints: 1 });
      if (estimate.fitsRecommendedBudget === false) {
        const proceed = window.confirm(`Estimated peak memory is ${estimate.estimatedPeakMb.toFixed(0)} MB, above the recommended ${estimate.recommendedBudgetMb?.toFixed(0) ?? "unknown"} MB budget. Continue anyway?`);
        if (!proceed) { statusMessage = "DC analysis cancelled by user"; return; }
      }
      statusMessage = "Assembling MNA system…";
      dcResult = await invoke<DcResult>("simulate_dc", { input: circuitPayload() });
      isDirty = true;
      statusMessage = "DC operating point converged · results not yet saved";
    } catch (error) {
      dcResult = null;
      errorMessage = String(error);
      statusMessage = "DC analysis failed";
    } finally {
      isRunning = false;
    }
  }

  async function runTransient() {
    if (isRunning) return;
    const stop = Number(timeStop);
    const step = Number(timeStep);
    if (!Number.isFinite(stop) || !Number.isFinite(step) || stop <= 0 || step <= 0) {
      errorMessage = "Enter a finite stop time and time step greater than zero.";
      return;
    }
    isRunning = true;
    errorMessage = "";
    dcResult = null;
    statusMessage = "Running backward-Euler transient solve…";
    try {
      const points = Math.ceil(stop / step) + 1;
      if (!Number.isFinite(points) || points > 1_000_001) {
        errorMessage = "This transient request exceeds the current 1,000,000-step solver limit. Increase the time step.";
        return;
      }
      const estimate = await invoke<SimulationEstimate>("estimate_simulation", {
        input: circuitPayload(), timePoints: points,
      });
      if (estimate.fitsRecommendedBudget === false) {
        const proceed = window.confirm(`Estimated peak memory is ${estimate.estimatedPeakMb.toFixed(0)} MB, above the recommended ${estimate.recommendedBudgetMb?.toFixed(0) ?? "unknown"} MB budget. Continue anyway?`);
        if (!proceed) { statusMessage = "Transient analysis cancelled by user"; return; }
      }
      transientResult = await invoke<TransientResult>("simulate_transient", {
        input: circuitPayload(), tStop: stop, tStep: step,
      });
      activeTrace = 0;
      isDirty = true;
      statusMessage = `Transient complete · ${transientResult.times.length - 1} time steps · results not yet saved`;
    } catch (error) {
      transientResult = null;
      errorMessage = String(error);
      statusMessage = "Transient analysis failed";
    } finally {
      isRunning = false;
    }
  }

  async function saveProject() {
    errorMessage = "";
    try {
      const path = projectPath ?? await save({
        defaultPath: `${projectName.replace(/[\\/:*?"<>|]/g, "_")}.eeproj`,
        filters: [{ name: "EEE Workbench Project", extensions: ["eeproj"] }],
      });
      if (!path) return;
      const savedPath = await invoke<string>("save_project", {
        path, name: projectName, circuit: circuitPayload(),
        simulationConfig: { tStop: Number(timeStop), tStep: Number(timeStep) },
        results: { dcResult, transientResult },
      });
      projectPath = savedPath;
      rememberProject(projectName, savedPath);
      isDirty = false;
      statusMessage = `Saved ${savedPath}`;
    } catch (error) {
      errorMessage = String(error);
    }
  }

  async function openProject(path?: string) {
    errorMessage = "";
    try {
      const selectedPath = path ?? await open({
        multiple: false,
        filters: [{ name: "EEE Workbench Project", extensions: ["eeproj"] }],
      });
      if (typeof selectedPath !== "string") return;
      const loaded = await invoke<ProjectResult>("load_project", { path: selectedPath });
      projectName = loaded.name;
      elements = loaded.circuit.elements ?? [];
      wires = loaded.circuit.wires ?? [];
      const savedResults = loaded.results as { dcResult?: DcResult | null; transientResult?: TransientResult | null } | null;
      dcResult = savedResults?.dcResult ?? null;
      transientResult = savedResults?.transientResult ?? null;
      activeTrace = 0;
      if (loaded.simulationConfig) {
        timeStop = String(loaded.simulationConfig.tStop);
        timeStep = String(loaded.simulationConfig.tStep);
      }
      projectPath = selectedPath;
      selectedId = null;
      idCounter = Math.max(elements.length + 1, 1);
      isDirty = false;
      rememberProject(projectName, selectedPath);
      statusMessage = loaded.recoveredFrom === "primary"
        ? `Opened ${projectName}`
        : `Recovered ${projectName} from ${loaded.recoveredFrom}`;
    } catch (error) {
      errorMessage = String(error);
    }
  }

  function rememberProject(name: string, path: string) {
    recentProjects = [{ name, path }, ...recentProjects.filter((item) => item.path !== path)].slice(0, 8);
    try { localStorage.setItem("eee-workbench-recent", JSON.stringify(recentProjects)); } catch { /* optional convenience */ }
  }

  function rangeOf(values: number[]) {
    if (!values.length) return { min: 0, max: 1 };
    let min = Number.POSITIVE_INFINITY;
    let max = Number.NEGATIVE_INFINITY;
    for (const value of values) { min = Math.min(min, value); max = Math.max(max, value); }
    if (Math.abs(max - min) < 1e-12) { min -= 1; max += 1; }
    const pad = (max - min) * 0.08;
    return { min: min - pad, max: max + pad };
  }

  function makeWaveformPath(times: number[], values: number[]): string {
    if (!times.length || !values.length) return "";
    const maxPoints = 1400;
    const stride = Math.max(1, Math.ceil(times.length / maxPoints));
    const range = rangeOf(values);
    const start = times[0];
    const end = times[times.length - 1] || 1;
    const width = 790;
    const height = 172;
    const points: string[] = [];
    for (let index = 0; index < times.length; index += stride) {
      const x = 42 + ((times[index] - start) / (end - start || 1)) * width;
      const y = 12 + (1 - (values[index] - range.min) / (range.max - range.min)) * height;
      points.push(`${x.toFixed(2)},${y.toFixed(2)}`);
    }
    const last = times.length - 1;
    const x = 42 + ((times[last] - start) / (end - start || 1)) * width;
    const y = 12 + (1 - (values[last] - range.min) / (range.max - range.min)) * height;
    if (last % stride !== 0) points.push(`${x.toFixed(2)},${y.toFixed(2)}`);
    return points.join(" ");
  }

  function formatEngineering(value: number): string {
    const absolute = Math.abs(value);
    if (absolute >= 1e6) return `${(value / 1e6).toPrecision(4)} M`;
    if (absolute >= 1e3) return `${(value / 1e3).toPrecision(4)} k`;
    if (absolute >= 1) return `${value.toPrecision(4)}`;
    if (absolute >= 1e-3) return `${(value * 1e3).toPrecision(4)} m`;
    if (absolute >= 1e-6) return `${(value * 1e6).toPrecision(4)} µ`;
    if (absolute >= 1e-9) return `${(value * 1e9).toPrecision(4)} n`;
    return `${value.toExponential(3)}`;
  }

  function unitFor(kind: ElementKind): string {
    return ({ resistor: "Ω", capacitor: "F", inductor: "H", voltage_source: "V", current_source: "A", diode: "A (Is)", ground: "—" } as Record<ElementKind, string>)[kind];
  }

  function symbolLabel(kind: ElementKind): string {
    return ({ resistor: "Resistor", capacitor: "Capacitor", inductor: "Inductor", voltage_source: "DC voltage source", current_source: "DC current source", diode: "Shockley diode", ground: "Ground" } as Record<ElementKind, string>)[kind];
  }

  function showResource(snapshot: ResourceSnapshot | null): string {
    if (!snapshot) return "Detecting…";
    if (snapshot.availableMemoryMb === null) return `${snapshot.logicalCpuCores} logical cores`;
    return `${(snapshot.availableMemoryMb / 1024).toFixed(1)} GB available · ${snapshot.logicalCpuCores} cores`;
  }

  onMount(() => {
    try {
      const recent = localStorage.getItem("eee-workbench-recent");
      if (recent) recentProjects = JSON.parse(recent);
    } catch { recentProjects = []; }

    const canvas = document.createElement("canvas");
    webgl2Available = Boolean(canvas.getContext("webgl2"));
    const updateResource = () => invoke<ResourceSnapshot>("resource_snapshot")
      .then((snapshot) => resource = snapshot)
      .catch(() => resource = null);
    updateResource();
    const resourceTimer = window.setInterval(updateResource, 5000);
    const autosaveTimer = window.setInterval(async () => {
      if (!projectPath || !isDirty || isRunning) return;
      try {
        await invoke("autosave_project", {
          path: projectPath, name: projectName, circuit: circuitPayload(),
          simulationConfig: { tStop: Number(timeStop), tStep: Number(timeStep) },
          results: { dcResult, transientResult },
        });
        statusMessage = "Autosave written";
      } catch { /* primary project remains untouched; status is surfaced on next save */ }
    }, 30000);
    return () => {
      window.clearInterval(resourceTimer);
      window.clearInterval(autosaveTimer);
    };
  });
</script>

<svelte:window onpointermove={moveDrag} onpointerup={endDrag} onkeydown={handleKeydown} />

<div class="app-shell">
  <header class="topbar">
    <div class="brand-mark"><span class="brand-symbol">∿</span><span>EEE <b>WORKBENCH</b></span></div>
    <div class="project-title">
      <span class="crumb">PROJECT /</span>
      <input aria-label="Project name" bind:value={projectName} oninput={markDirty} />
      {#if isDirty}<span class="dirty-dot" title="Unsaved changes"></span>{/if}
    </div>
    <div class="top-actions">
      <button class="quiet-button" onclick={newProject}>New</button>
      <button class="quiet-button" onclick={() => showRecent = !showRecent}>Recent</button>
      <button class="quiet-button" onclick={() => openProject()}>Open</button>
      <button class="save-button" onclick={saveProject}>Save project <span>⌘ S</span></button>
    </div>
  </header>

  {#if showRecent}
    <div class="recent-popover">
      <div class="popover-heading">Recent projects <button onclick={() => showRecent = false}>×</button></div>
      {#if recentProjects.length}
        {#each recentProjects as recent}
          <button class="recent-item" onclick={() => { showRecent = false; openProject(recent.path); }}>
            <span>{recent.name}</span><small>{recent.path}</small>
          </button>
        {/each}
      {:else}
        <p class="muted small">Projects you open or save will appear here.</p>
      {/if}
    </div>
  {/if}

  <div class="workspace">
    <aside class="left-rail">
      <div class="rail-label">WORKSPACE</div>
      <button class="studio-link active"><span class="studio-glyph">⌁</span><span>Circuit Studio</span><i>01</i></button>
      <div class="rail-label secondary-label">OTHER STUDIOS</div>
      <div class="planned-link"><span>◫</span> Analog & Semiconductor <em>Planned</em></div>
      <div class="planned-link"><span>◉</span> Electrical Machines <em>Planned</em></div>
      <div class="planned-link"><span>⌂</span> Power Systems <em>Planned</em></div>
      <div class="planned-link"><span>✳</span> Electromagnetics <em>Planned</em></div>
      <div class="planned-link"><span>⌁</span> Control Systems <em>Planned</em></div>
      <div class="planned-link"><span>∿</span> Signal Processing <em>Planned</em></div>
      <div class="planned-link"><span>▦</span> Embedded & Digital <em>Planned</em></div>
      <div class="rail-bottom">
        <div class="project-explorer">
          <div class="rail-label">PROJECT EXPLORER</div>
          <div class="tree-row"><span class="tree-chevron">⌄</span><span class="folder-icon">▰</span>{projectName}</div>
          <div class="tree-row indent"><span class="file-icon">◇</span> Circuit schematic</div>
          <div class="tree-row indent"><span class="file-icon">▤</span> Simulation results</div>
        </div>
        <div class="system-status"><span class="status-led"></span><span>LOCAL COMPUTE</span><strong>{resource?.memoryPressure === "high" ? "MEMORY HIGH" : "ONLINE"}</strong></div>
      </div>
    </aside>

    <main class="main-area">
      <div class="studio-header">
        <div>
          <div class="eyebrow">ENGINEERING STUDIO / 01</div>
          <h1>Circuit Studio</h1>
          <p>Build a schematic. Solve the circuit. Inspect the response.</p>
        </div>
        <div class="studio-meta"><span class="chip">MNA SOLVER</span><span class="chip">SI UNITS</span><span class="chip">CPU</span></div>
      </div>

      <div class="editor-toolbar">
        <div class="tool-group">
          <span class="tool-caption">PLACE COMPONENT</span>
          <button class="tool-button" title="Resistor" onclick={() => addElement("resistor")}><b>R</b><span>Resistor</span></button>
          <button class="tool-button" title="Capacitor" onclick={() => addElement("capacitor")}><b>C</b><span>Capacitor</span></button>
          <button class="tool-button" title="Inductor" onclick={() => addElement("inductor")}><b>L</b><span>Inductor</span></button>
          <button class="tool-button" title="DC voltage source" onclick={() => addElement("voltage_source")}><b>V</b><span>Voltage</span></button>
          <button class="tool-button" title="DC current source" onclick={() => addElement("current_source")}><b>I</b><span>Current</span></button>
          <button class="tool-button" title="Shockley diode" onclick={() => addElement("diode")}><b>D</b><span>Diode</span></button>
          <button class="tool-button ground-tool" title="Ground" onclick={() => addElement("ground")}><b>⏚</b><span>Ground</span></button>
        </div>
        <div class="tool-divider"></div>
        <div class="tool-group actions-group">
          <button class:chosen={wireMode} class="wire-button" onclick={toggleWireMode}><span>⌁</span> {wireMode ? "Cancel wire" : "Wire"}</button>
          <button class="icon-button" title="Delete selected" disabled={!selectedId} onclick={removeSelected}>⌫</button>
          <button class="example-button" onclick={loadDividerExample}>Load divider example</button>
        </div>
      </div>

      <div class="editor-grid">
        <section class="canvas-panel">
          <div class="panel-topline">
            <div><span class="panel-index">01</span><b>SCHEMATIC CANVAS</b><span class="canvas-hint">Drag components · click terminals to wire</span></div>
            <div class="canvas-tools"><span class="zoom-label">100%</span><span class="canvas-coordinate">{elements.length} PARTS / {wires.length} WIRES</span></div>
          </div>
          <div class="canvas-wrap">
            <svg bind:this={svgElement} class="schematic" viewBox={`0 0 ${canvasWidth} ${canvasHeight}`} role="img" aria-label="Circuit schematic editor" onclick={clearSelection}>
              <defs>
                <pattern id="small-grid" width="20" height="20" patternUnits="userSpaceOnUse">
                  <circle cx="1" cy="1" r="1" fill="#29343d" />
                </pattern>
                <pattern id="major-grid" width="100" height="100" patternUnits="userSpaceOnUse">
                  <rect width="100" height="100" fill="url(#small-grid)" />
                  <circle cx="1" cy="1" r="1.4" fill="#394752" />
                </pattern>
              </defs>
              <rect width={canvasWidth} height={canvasHeight} fill="url(#major-grid)" />
              {#each wires as wire, index}
                <path class="wire-line" class:wire-selected={wireStart && (samePin(wire.from, wireStart) || samePin(wire.to, wireStart))} d={wireD(wire)} />
              {/each}
              {#each elements as element (element.id)}
                <g class="component" class:selected={selectedId === element.id} transform={`translate(${element.x} ${element.y})`} onpointerdown={(event) => beginDrag(event, element.id)}>
                  {#if element.kind === "resistor"}
                    <path class="symbol" d="M -48 0 L -34 0 L -27 -10 L -15 10 L -3 -10 L 9 10 L 21 -10 L 34 0 L 48 0" />
                  {:else if element.kind === "capacitor"}
                    <path class="symbol" d="M -48 0 L -9 0 M -9 -18 L -9 18 M 9 -18 L 9 18 M 9 0 L 48 0" />
                  {:else if element.kind === "inductor"}
                    <path class="symbol" d="M -48 0 L -30 0 C -30 -23 -10 -23 -10 0 C -10 -23 10 -23 10 0 C 10 -23 30 -23 30 0 L 48 0" />
                  {:else if element.kind === "voltage_source"}
                    <path class="symbol" d="M -48 0 L -25 0 M 25 0 L 48 0" /><circle class="symbol" cx="0" cy="0" r="25" /><path class="polarity" d="M -8 -8 L 8 -8 M 0 -16 L 0 0 M -8 8 L 8 8" />
                  {:else if element.kind === "current_source"}
                    <path class="symbol" d="M -48 0 L -25 0 M 25 0 L 48 0" /><circle class="symbol" cx="0" cy="0" r="25" /><path class="polarity" d="M 0 13 L 0 -13 M -7 -5 L 0 -13 L 7 -5" />
                  {:else if element.kind === "diode"}
                    <path class="symbol" d="M -48 0 L -14 0 M 14 0 L 48 0 M -14 -17 L -14 17 L 14 0 Z M 14 -19 L 14 19" />
                  {:else}
                    <path class="ground-symbol" d="M 0 -16 L 0 0 M -20 0 L 20 0 M -13 8 L 13 8 M -6 16 L 6 16" />
                  {/if}
                  <text class="component-label" x="0" y={element.kind === "ground" ? 36 : -31} text-anchor="middle">{element.label}</text>
                  {#if element.kind !== "ground"}
                    <text class="component-value" x="0" y="39" text-anchor="middle">{formatEngineering(element.value)} {unitFor(element.kind)}</text>
                    <circle class="terminal terminal-hit" cx="-48" cy="0" r="5" onclick={(event) => clickTerminal(element.id, "a", event)} />
                    <circle class="terminal terminal-hit" cx="48" cy="0" r="5" onclick={(event) => clickTerminal(element.id, "b", event)} />
                  {:else}
                    <circle class="terminal terminal-hit" cx="0" cy="-16" r="5" onclick={(event) => clickTerminal(element.id, "a", event)} />
                  {/if}
                  <rect class="component-hit" x="-56" y="-45" width="112" height="94" rx="9" />
                </g>
              {/each}
              {#if wireMode && wireStart}
                <text class="wire-instruction" x="22" y="28">START: {getElement(wireStart.componentId)?.label} · {wireStart.terminal.toUpperCase()} — choose destination terminal</text>
              {/if}
              {#if elements.length === 0}
                <g class="empty-canvas" transform="translate(450 275)">
                  <circle cx="0" cy="-42" r="34" fill="#1a2730" stroke="#384752" />
                  <text x="0" y="-31" text-anchor="middle">⌁</text>
                  <text x="0" y="20" text-anchor="middle">Your schematic starts here</text>
                  <text class="empty-sub" x="0" y="48" text-anchor="middle">Place a component above, or load the voltage-divider example</text>
                </g>
              {/if}
            </svg>
          </div>
          <div class="canvas-footer">
            <span><i class="legend-wire"></i> Electrical connection</span>
            <span><i class="legend-node"></i> Component terminal</span>
            <span class="footer-right">GRID 20 PX · SNAP ENABLED</span>
          </div>
        </section>

        <aside class="properties-panel">
          <div class="panel-topline"><div><span class="panel-index">02</span><b>PROPERTIES</b></div><span class="panel-kebab">•••</span></div>
          {#if selectedElement}
            <div class="property-content">
              <div class="component-preview">
                <div class="preview-symbol">{kindPrefix(selectedElement.kind)}</div>
                <div><b>{selectedElement.label}</b><small>{symbolLabel(selectedElement.kind)}</small></div>
              </div>
              <label class="field-label">REFERENCE DESIGNATOR</label>
              <input class="property-input" value={selectedElement.label} oninput={(event) => updateSelectedLabel((event.currentTarget as HTMLInputElement).value)} />
              {#if selectedElement.kind !== "ground"}
                <label class="field-label">COMPONENT VALUE <span>{unitFor(selectedElement.kind)}</span></label>
                <div class="value-entry"><input bind:value={valueEditor} onkeydown={(event) => event.key === "Enter" && commitSelectedValue()} onblur={commitSelectedValue} /><span>{unitFor(selectedElement.kind)}</span></div>
                <small class="field-help">Engineering notation accepted · e.g. 4.7k, 100nF, 12mV</small>
              {:else}
                <div class="ground-note">Ground defines the circuit's 0 V reference node.</div>
              {/if}
              <div class="property-divider"></div>
              <div class="property-section-title">SCHEMATIC POSITION</div>
              <div class="position-grid"><label>X <input type="number" value={Math.round(selectedElement.x)} readonly /></label><label>Y <input type="number" value={Math.round(selectedElement.y)} readonly /></label></div>
              <div class="property-divider"></div>
              <div class="property-section-title">TERMINALS</div>
              <div class="terminal-row"><span class="terminal-name">A</span><span>Positive / reference side</span></div>
              {#if selectedElement.kind !== "ground"}<div class="terminal-row"><span class="terminal-name">B</span><span>Negative / return side</span></div>{/if}
              <div class="property-note">Current direction is defined from terminal A to terminal B.</div>
              <button class="remove-component" onclick={removeSelected}>Remove component</button>
            </div>
          {:else}
            <div class="properties-empty"><div class="empty-crosshair">＋</div><b>No component selected</b><p>Select a component on the canvas to inspect and edit its parameters.</p></div>
          {/if}
        </aside>
      </div>

      <section class="simulation-panel">
        <div class="simulation-heading">
          <div><span class="panel-index">03</span><b>SIMULATION & RESULTS</b><span class="simulation-sub">Real Rust MNA engine</span></div>
          <div class="simulation-controls">
            <button class="run-button" disabled={isRunning} onclick={runDc}><span>▶</span> Run DC</button>
            <label class="time-field">T STOP <input bind:value={timeStop} /></label>
            <label class="time-field">STEP <input bind:value={timeStep} /></label>
            <button class="transient-button" disabled={isRunning} onclick={runTransient}>Run transient ↗</button>
          </div>
        </div>
        {#if errorMessage}
          <div class="error-banner"><span>!</span>{errorMessage}</div>
        {/if}
        {#if dcResult}
          <div class="results-grid">
            <div class="result-section">
              <div class="result-label">NODE VOLTAGES <span>DC OPERATING POINT</span></div>
              <table><thead><tr><th>NODE</th><th>VOLTAGE</th></tr></thead><tbody>
                {#each dcResult.nodeVoltages as node}<tr><td>{node.label}</td><td class="number-cell">{node.voltage.toFixed(6)} V</td></tr>{/each}
              </tbody></table>
            </div>
            <div class="result-section">
              <div class="result-label">DEVICE CURRENTS <span>A → B</span></div>
              <table><thead><tr><th>DEVICE</th><th>CURRENT</th></tr></thead><tbody>
                {#each dcResult.deviceCurrents as device}<tr><td>{device.label}</td><td class="number-cell">{formatEngineering(device.current)} A</td></tr>{/each}
              </tbody></table>
            </div>
          </div>
        {:else if transientResult}
          <div class="waveform-layout">
            <div class="waveform-sidebar">
              <div class="result-label">VOLTAGE TRACES</div>
              {#each transientResult.traces as trace, index}
                <button class:trace-active={activeTrace === index} class="trace-select" onclick={() => activeTrace = index}><span class="trace-line"></span>{trace.label}</button>
              {/each}
              <div class="trace-stat"><small>FINAL VALUE</small><b>{selectedWaveform ? formatEngineering(selectedWaveform.values.at(-1) ?? 0) : "—"} V</b></div>
              <div class="trace-stat"><small>SAMPLES</small><b>{transientResult.times.length.toLocaleString()}</b></div>
            </div>
            <div class="waveform-main">
              <div class="result-label">TRANSIENT RESPONSE <span>BACKWARD EULER</span></div>
              {#if selectedWaveform}
                <svg class="waveform" viewBox="0 0 880 210" role="img" aria-label={`${selectedWaveform.label} transient waveform`}>
                  {#each [0, 1, 2, 3, 4] as tick}
                    <line x1="42" x2="832" y1={12 + tick * 43} y2={12 + tick * 43} class="chart-gridline" />
                  {/each}
                  <polyline points={waveformPath} class="waveform-line" />
                  <text x="42" y="205" class="chart-axis">0 s</text>
                  <text x="832" y="205" text-anchor="end" class="chart-axis">{transientResult.times.at(-1)?.toPrecision(4)} s</text>
                  <text x="12" y="13" class="chart-axis">{waveformRange.max.toPrecision(3)} V</text>
                  <text x="12" y="185" class="chart-axis">{waveformRange.min.toPrecision(3)} V</text>
                </svg>
              {/if}
            </div>
          </div>
        {:else}
          <div class="results-empty"><div class="results-icon">∿</div><div><b>Simulation results will appear here</b><p>Run a DC operating point or transient analysis to calculate node voltages and waveforms from this schematic.</p></div><div class="solver-ready"><span></span> SOLVER READY</div></div>
        {/if}
      </section>

      <footer class="statusbar">
        <div><span class="status-led"></span>{statusMessage}</div>
        <div class="status-metrics"><span>RAM {showResource(resource)}</span><span>CPU {resource?.logicalCpuCores ?? "—"} CORES</span><span>WEBGL2 {webgl2Available ? "AVAILABLE" : "FALLBACK"}</span><span>EEE CORE 0.1.0</span></div>
      </footer>
    </main>
  </div>
</div>
