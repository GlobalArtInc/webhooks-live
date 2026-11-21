const lastSelected = new Map<string, string>();

export function rememberSelection(endpointId: string, captureId: string): void {
  lastSelected.set(endpointId, captureId);
}

export function rememberedSelection(endpointId: string): string | null {
  return lastSelected.get(endpointId) ?? null;
}

export function forgetSelection(endpointId: string): void {
  lastSelected.delete(endpointId);
}
