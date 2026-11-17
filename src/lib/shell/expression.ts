/**
 * Evaluates a numeric field's text: numbers, `+ - * /`, unary minus and
 * parentheses, so `120/2` or `(30 + 4) * 2` work. Returns null for anything
 * else, including division by zero.
 */
export function evaluate(text: string): number | null {
  const tokens = text.match(/\d*\.?\d+|\S/g) ?? [];
  let position = 0;
  const peek = () => tokens[position];

  // expression := term (("+" | "-") term)*
  function expression(): number {
    let value = term();
    while (peek() === "+" || peek() === "-") {
      const operator = tokens[position++];
      const right = term();
      value = operator === "+" ? value + right : value - right;
    }
    return value;
  }

  // term := unary (("*" | "/") unary)*
  function term(): number {
    let value = unary();
    while (peek() === "*" || peek() === "/") {
      const operator = tokens[position++];
      const right = unary();
      value = operator === "*" ? value * right : value / right;
    }
    return value;
  }

  // unary := ("-" | "+") unary | number | "(" expression ")"
  function unary(): number {
    const token = tokens[position++];
    if (token === "-") return -unary();
    if (token === "+") return unary();
    if (token === "(") {
      const value = expression();
      if (tokens[position++] !== ")") return NaN;
      return value;
    }
    return token !== undefined && /^\d*\.?\d+$/.test(token) ? Number(token) : NaN;
  }

  if (tokens.length === 0) return null;
  const value = expression();
  return position === tokens.length && Number.isFinite(value) ? value : null;
}
