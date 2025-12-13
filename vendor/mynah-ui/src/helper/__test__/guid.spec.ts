import { generateUID } from '../guid';

describe('generateUID', () => {
  it('should generate a unique identifier', () => {
    const uid1 = generateUID();
    const uid2 = generateUID();

    expect(uid1).toBeDefined();
    expect(uid2).toBeDefined();
    expect(uid1).not.toBe(uid2);
    expect(typeof uid1).toBe('string');
    expect(typeof uid2).toBe('string');
  });

  it('should generate UIDs of consistent format', () => {
    const uid = generateUID();

    // Should be a string with some length
    expect(uid.length).toBeGreaterThan(0);

    // Should contain alphanumeric characters
    expect(uid).toMatch(/^[a-zA-Z0-9]+$/);
  });

  it('should generate different UIDs on multiple calls', () => {
    const uids = new Set();

    // Generate 100 UIDs and ensure they're all unique
    for (let i = 0; i < 100; i++) {
      uids.add(generateUID());
    }

    expect(uids.size).toBe(100);
  });
});
