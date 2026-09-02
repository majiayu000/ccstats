const integer = new Intl.NumberFormat("en-US");

export function formatTokens(value: number) {
  return integer.format(value);
}

export function formatCost(value: number | null, currency: string) {
  if (value === null) return "Unknown";
  try {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency,
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(value);
  } catch {
    return "Invalid currency";
  }
}

export function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
