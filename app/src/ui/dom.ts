export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> & { className?: string } = {},
  children: (Node | string)[] = [],
): HTMLElementTagNameMap[K] {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children);
  return node;
}

export function fillSelect(select: HTMLSelectElement, values: string[], selected: string): void {
  select.replaceChildren(...values.map((v) => el("option", { value: v, textContent: v })));
  select.value = values.includes(selected) ? selected : (values[0] ?? "");
}
