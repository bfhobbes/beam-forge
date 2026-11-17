// What the burn view and protocol inspector both point at: the G-code line
// picked in either one is highlighted in the other.

export const selection = $state<{ line: number | null }>({ line: null });
