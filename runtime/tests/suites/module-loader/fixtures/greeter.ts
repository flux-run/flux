// Default export — tests default import syntax
export default class Greeter {
  constructor(private readonly greeting: string) {}
  greet(name: string): string { return `${this.greeting}, ${name}!`; }
}
