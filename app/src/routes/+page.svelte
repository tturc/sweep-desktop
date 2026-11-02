<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  type Root = { path: string; kind: string; items: number };
  type Suggestion = {
    name: string;
    kind: string;
    age_days: number;
    lane: string;
    confidence: string;
    reasons: string[];
  };
  type Board = {
    roots: Root[];
    lanes: Record<string, number>;
    suggestions: Suggestion[];
    last_plan_status: string | null;
  };
  type PlanOp = { seq: number; kind: string; src: string; dst: string; reason: string };
  type Plan = {
    version: number;
    plan_id: number | null;
    workspace: string;
    created_at: string;
    ops: PlanOp[];
  };

  const lanes = [
    { id: "bench", label: "Bench", hint: "This week", color: "#f59e0b" },
    { id: "kits", label: "Kits", hint: "Yours, rerun", color: "#34d399" },
    { id: "shelf", label: "Shelf", hint: "Keep, not now", color: "#60a5fa" },
    { id: "cold", label: "Cold", hint: "Record", color: "#94a3b8" },
    { id: "inbox", label: "Inbox", hint: "Decide later", color: "#fbbf24" },
  ];

  let board: Board | null = $state(null);
  let plan: Plan | null = $state(null);
  let busy = $state(false);
  let message = $state("");

  async function refresh() {
    try {
      board = await invoke<Board>("get_board");
    } catch (error) {
      message = String(error);
    }
  }

  async function scan() {
    busy = true;
    try {
      board = await invoke<Board>("scan_roots");
      message = "Scanned.";
    } catch (error) {
      message = String(error);
    }
    busy = false;
  }

  async function makePlan(lane?: string) {
    busy = true;
    try {
      plan = await invoke<Plan>("create_plan", { lane: lane ?? null });
      message = `${plan.ops.length} moves ready.`;
    } catch (error) {
      message = String(error);
    }
    busy = false;
  }

  async function applyPlan() {
    if (!plan) return;
    busy = true;
    try {
      message = await invoke<string>("apply_plan", { plan });
      plan = null;
      await refresh();
    } catch (error) {
      message = String(error);
    }
    busy = false;
  }

  async function undo() {
    busy = true;
    try {
      message = await invoke<string>("undo_last");
      await refresh();
    } catch (error) {
      message = String(error);
    }
    busy = false;
  }

  onMount(refresh);
</script>

<main class="shell">
  <header class="top">
    <div class="brand">
      <div class="mark">S</div>
      <div>
        <h1>Sweep</h1>
        <p>Your desktop, swept. Every move is reversible.</p>
      </div>
    </div>
    <div class="actions">
      <button onclick={scan} disabled={busy}>Scan</button>
      <button onclick={() => makePlan()} disabled={busy}>Plan all</button>
      <button class="undo" onclick={undo} disabled={busy}>Undo</button>
    </div>
  </header>

  <section class="roots">
    {#each board?.roots ?? [] as root}
      <div class="root">
        <span class="root-kind">{root.kind}</span>
        <strong>{root.items}</strong>
        <span class="root-path">{root.path}</span>
      </div>
    {/each}
    {#if (board?.roots?.length ?? 0) === 0}
      <div class="empty">No roots yet. Run <code>sweep init</code> in the CLI, then hit Scan.</div>
    {/if}
  </section>

  <section class="board">
    {#each lanes as lane}
      <article class="lane" style={`--lane:${lane.color}`}>
        <header class="lane-head">
          <div>
            <h2>{lane.label}</h2>
            <p>{lane.hint}</p>
          </div>
          <span class="count">{board?.lanes[lane.id] ?? 0}</span>
        </header>
        <div class="cards">
          {#each (board?.suggestions ?? []).filter((item) => item.lane === lane.id).slice(0, 5) as item}
            <div class="card">
              <strong title={item.name}>{item.name}</strong>
              <div class="meta">
                <span>{item.kind}</span>
                <span>{item.age_days}d</span>
                <span class={`conf ${item.confidence}`}>{item.confidence}</span>
              </div>
              <p>{item.reasons.join(" · ")}</p>
            </div>
          {/each}
          {#if (board?.suggestions ?? []).filter((item) => item.lane === lane.id).length === 0}
            <div class="blank">nothing here</div>
          {/if}
          {#if (board?.lanes[lane.id] ?? 0) > 5}
            <div class="more-count">+{(board?.lanes[lane.id] ?? 0) - 5} more</div>
          {/if}
        </div>
        <button class="lane-plan" onclick={() => makePlan(lane.id)} disabled={busy}>
          Plan {lane.label}
        </button>
      </article>
    {/each}
  </section>

  {#if plan}
    <aside class="drawer">
      <header>
        <div>
          <h2>Plan preview</h2>
          <p>{plan.ops.length} moves, dry run validated</p>
        </div>
        <button class="close" onclick={() => (plan = null)}>Close</button>
      </header>
      <div class="ops">
        {#each plan.ops.slice(0, 10) as op}
          <div class="op">
            <span class="seq">{op.seq}</span>
            <div>
              <strong>{op.src.split("\\").pop()}</strong>
              <small>{op.dst}</small>
            </div>
          </div>
        {/each}
        {#if plan.ops.length > 10}
          <p class="more">+ {plan.ops.length - 10} more</p>
        {/if}
      </div>
      <button class="apply" onclick={applyPlan} disabled={busy}>Apply {plan.ops.length} moves</button>
    </aside>
  {/if}

  <footer>
    <span class:bad={message.toLowerCase().includes("error") || message.toLowerCase().includes("failed")}>
      {message}
    </span>
    <span>{board?.last_plan_status ? `last plan: ${board.last_plan_status}` : ""}</span>
  </footer>
</main>

<style>
  :global(body) {
    margin: 0;
    background: #0b0f14;
    color: #e5e7eb;
    font-family: Inter, system-ui, -apple-system, sans-serif;
  }

  :global(*) {
    box-sizing: border-box;
  }

  .shell {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    padding: 26px 30px;
  }

  .top {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 24px;
  }

  .brand {
    display: flex;
    gap: 14px;
    align-items: center;
  }

  .mark {
    width: 44px;
    height: 44px;
    border-radius: 12px;
    background: linear-gradient(135deg, #f59e0b, #ef4444);
    display: grid;
    place-items: center;
    font-weight: 800;
    color: #0b0f14;
    font-size: 20px;
  }

  h1 {
    margin: 0;
    font-size: 24px;
    letter-spacing: -0.02em;
  }

  .brand p {
    margin: 2px 0 0;
    color: #94a3b8;
    font-size: 13px;
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  button {
    background: #1e293b;
    color: #e2e8f0;
    border: 1px solid #334155;
    border-radius: 8px;
    padding: 8px 12px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: border-color 0.15s, color 0.15s, opacity 0.15s;
  }

  button:hover:not(:disabled) {
    border-color: #64748b;
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .roots {
    display: flex;
    gap: 10px;
    margin: 22px 0 16px;
    flex-wrap: wrap;
  }

  .root {
    display: flex;
    align-items: center;
    gap: 10px;
    background: #111827;
    border: 1px solid #1f2937;
    border-radius: 10px;
    padding: 8px 12px;
    font-size: 12px;
  }

  .root strong {
    font-size: 16px;
    color: #f8fafc;
  }

  .root-kind {
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #94a3b8;
    font-size: 10px;
  }

  .root-path {
    color: #64748b;
  }

  .empty {
    color: #64748b;
    font-size: 12px;
    padding: 8px 0;
  }

  .empty code {
    color: #fbbf24;
  }

  .board {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: 12px;
    flex: 1;
    min-height: 0;
  }

  .lane {
    background: #0f172a;
    border: 1px solid #1e293b;
    border-top: 2px solid var(--lane);
    border-radius: 12px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    min-height: 300px;
  }

  .lane-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
  }

  .lane-head h2 {
    margin: 0;
    font-size: 15px;
    color: var(--lane);
  }

  .lane-head p {
    margin: 2px 0 0;
    color: #64748b;
    font-size: 11px;
  }

  .count {
    font-size: 20px;
    font-weight: 700;
    color: #e2e8f0;
  }

  .cards {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 12px;
    flex: 1;
    overflow: auto;
  }

  .card {
    background: #111c2e;
    border: 1px solid #1e293b;
    border-radius: 8px;
    padding: 8px;
  }

  .card strong {
    display: block;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .meta {
    display: flex;
    gap: 6px;
    margin-top: 4px;
    font-size: 10px;
    color: #94a3b8;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .conf.high {
    color: #34d399;
  }

  .conf.medium {
    color: #fbbf24;
  }

  .conf.low {
    color: #64748b;
  }

  .card p {
    margin: 6px 0 0;
    font-size: 10px;
    color: #64748b;
    line-height: 1.4;
  }

  .blank {
    color: #334155;
    font-size: 11px;
    text-align: center;
    padding: 20px 0;
  }

  .more-count {
    color: #475569;
    font-size: 10px;
    text-align: center;
    padding: 4px 0;
  }

  .lane-plan {
    margin-top: 10px;
    background: transparent;
    border: 1px solid #1e293b;
    color: #94a3b8;
    font-size: 11px;
    padding: 6px;
    border-radius: 6px;
  }

  .lane-plan:hover:not(:disabled) {
    border-color: var(--lane);
    color: var(--lane);
  }

  .drawer {
    position: fixed;
    right: 24px;
    bottom: 24px;
    width: 440px;
    max-height: 62vh;
    overflow: auto;
    background: #0f172a;
    border: 1px solid #334155;
    border-radius: 14px;
    padding: 16px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.55);
  }

  .drawer header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
  }

  .drawer h2 {
    margin: 0;
    font-size: 16px;
  }

  .drawer p {
    margin: 2px 0 0;
    color: #94a3b8;
    font-size: 12px;
  }

  .close {
    background: transparent;
    border: 0;
    color: #94a3b8;
    padding: 2px 6px;
  }

  .ops {
    margin: 12px 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .op {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    font-size: 11px;
  }

  .seq {
    color: #475569;
    width: 20px;
    text-align: right;
    flex: none;
  }

  .op strong {
    display: block;
    color: #e2e8f0;
  }

  .op small {
    display: block;
    color: #64748b;
    word-break: break-all;
  }

  .more {
    color: #64748b;
    font-size: 11px;
  }

  .apply {
    width: 100%;
    background: #f59e0b;
    color: #0b0f14;
    font-weight: 700;
    border: 0;
    border-radius: 8px;
    padding: 10px;
  }

  footer {
    display: flex;
    justify-content: space-between;
    margin-top: 14px;
    color: #64748b;
    font-size: 11px;
    gap: 16px;
  }

  .bad {
    color: #f87171;
  }

  @media (max-width: 1100px) {
    .board {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
</style>
