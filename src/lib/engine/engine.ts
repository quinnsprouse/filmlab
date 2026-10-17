import * as wasm from "@engine/engine.js";
import wasmUrl from "@engine/engine_bg.wasm?url";

import type {
  ControlSpec,
  DevelopSettings,
  ExportOptions,
  ExportPlan,
  Facts,
  FilmSettings,
  PhotoState,
} from "./types";

let booted: Promise<WebAssembly.Module> | undefined;

async function compileAndStart(): Promise<WebAssembly.Module> {
  const module = await WebAssembly.compileStreaming(fetch(wasmUrl));
  await wasm.default({ module_or_path: module });
  return module;
}

export function bootEngine(): Promise<WebAssembly.Module> {
  booted ??= compileAndStart();
  return booted;
}

const json = JSON.stringify;

export function controlSpecs(): ControlSpec[] {
  return JSON.parse(wasm.controls());
}

export function controlValues(settings: DevelopSettings): Record<string, number> {
  return JSON.parse(wasm.controlValues(json(settings)));
}

export interface WhiteBalanceView {
  mode: WhiteBalanceMode;
  temp: number;
  tint: number;
  kelvin: boolean;
}

export type WhiteBalanceMode =
  | "asShot"
  | "auto"
  | "daylight"
  | "cloudy"
  | "shade"
  | "tungsten"
  | "fluorescent"
  | "flash"
  | "custom";

export function whiteBalance(facts: Facts, settings: DevelopSettings): WhiteBalanceView {
  return JSON.parse(wasm.whiteBalance(json(facts), json(settings)));
}

export function applyProfile(
  settings: DevelopSettings,
  partial: DevelopSettings,
  amount: number,
): DevelopSettings {
  return JSON.parse(wasm.applyProfile(json(settings), json(partial), amount));
}

export function extractProfile(settings: DevelopSettings, groups: string[]): DevelopSettings {
  return JSON.parse(wasm.extractProfile(json(settings), json(groups)));
}

export interface MapRequest {
  width: number;
  height: number;
  applyCrop: boolean;
  points: [number, number][];
  toImage: boolean;
}

export function mapPoints(
  facts: Facts,
  settings: DevelopSettings,
  request: MapRequest,
): [number, number][] {
  return JSON.parse(wasm.mapPoints(json(facts), json(settings), json(request)));
}

export function filmOf(xmp: string): FilmSettings | undefined {
  const film = wasm.filmOf(xmp);
  return film ? JSON.parse(film) : undefined;
}

export function nativeSize(facts: Facts, settings: DevelopSettings): [number, number] {
  return JSON.parse(wasm.outputSize(json(facts), json(settings)));
}

export function writeSidecar(facts: Facts, state: PhotoState, existing: string | null): string {
  return wasm.writeSidecar(json(facts), json(state), existing);
}

export function exportPlan(facts: Facts, state: PhotoState, options: ExportOptions): ExportPlan {
  return JSON.parse(wasm.exportPlan(json(facts), json(state), json(options)));
}

let commands: wasm.Commands | undefined;

export function openCommands(facts: Facts, settings: DevelopSettings): void {
  commands ??= new wasm.Commands();
  commands.open(json(facts), json(settings));
}

export function attachPixels(bytes: Uint8Array): void {
  commands?.attach(bytes);
}

export function detachPixels(): void {
  commands?.detach();
}

export interface CommandResult {
  result: unknown;
  settings: DevelopSettings;
}

export function runCommand(command: string, params: Record<string, unknown>): CommandResult {
  if (!commands) {
    throw new Error("Open a photo before editing it.");
  }
  return JSON.parse(commands.run(command, json(params)));
}

