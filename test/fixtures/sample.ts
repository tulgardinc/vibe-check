// Regular function declaration
export function calculateTotal(items: number[], taxRate: number): number {
  const subtotal = items.reduce((sum, item) => sum + item, 0);
  const tax = subtotal * taxRate;
  return subtotal + tax;
}

// Arrow function assigned to const
export const formatCurrency = (amount: number, currency: string): string => {
  const formatter = new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency,
  });
  return formatter.format(amount);
};

// Class with methods
class UserService {
  private users: Map<string, { name: string; email: string }> = new Map();

  addUser(name: string, email: string): void {
    const id = Math.random().toString(36).slice(2);
    this.users.set(id, { name, email });
    console.log(`User ${name} added with id ${id}`);
  }

  getUser(id: string): { name: string; email: string } | undefined {
    return this.users.get(id);
  }
}

// Generator function
function* generateIds(prefix: string): Generator<string> {
  let counter = 0;
  while (true) {
    yield `${prefix}-${counter++}`;
    console.log('Generated id');
  }
}

// Function expression
const processData = function (data: string[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const item of data) {
    counts.set(item, (counts.get(item) ?? 0) + 1);
  }
  return counts;
};

// Too small — should be skipped (less than 3 lines)
function tiny(x: number): number { return x * 2; }

// Anonymous callback — should be skipped
const arr = [1, 2, 3].map(x => x * 2);
