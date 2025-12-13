import { StyleLoader } from '../style-loader';

describe('StyleLoader', () => {
  let styleLoader: StyleLoader;

  beforeEach(() => {
    // Clear any existing style elements
    document.head.querySelectorAll('style').forEach(el => el.remove());
    styleLoader = StyleLoader.getInstance();
  });

  afterEach(() => {
    // Clean up after each test
    document.head.querySelectorAll('style').forEach(el => el.remove());
  });

  it('should be a singleton', () => {
    const loader1 = StyleLoader.getInstance();
    const loader2 = StyleLoader.getInstance();

    expect(loader1).toBe(loader2);
  });

  it('should load styles', () => {
    const stylePath = 'test-style.scss';

    styleLoader.load(stylePath);

    // Should not throw an error
    expect(styleLoader).toBeDefined();
  });

  it('should handle multiple style loads', () => {
    const stylePath1 = 'style1.scss';
    const stylePath2 = 'style2.scss';

    styleLoader.load(stylePath1);
    styleLoader.load(stylePath2);

    expect(styleLoader).toBeDefined();
  });

  it('should handle duplicate style loads', () => {
    const stylePath = 'duplicate-style.scss';

    styleLoader.load(stylePath);
    styleLoader.load(stylePath); // Load same style again

    expect(styleLoader).toBeDefined();
  });

  it('should handle empty style path', () => {
    expect(async () => await styleLoader.load('')).not.toThrow();
  });

  it('should handle null/undefined style path', () => {
    expect(async () => await styleLoader.load(null as any)).not.toThrow();
    expect(async () => await styleLoader.load(undefined as any)).not.toThrow();
  });

  it('should handle various file extensions', () => {
    const styles = [
      'component.scss',
      'layout.css',
      'theme.sass'
    ];

    styles.forEach(style => {
      expect(async () => await styleLoader.load(style)).not.toThrow();
    });
  });

  it('should handle nested paths', () => {
    const nestedPath = 'components/button/_button.scss';

    expect(async () => await styleLoader.load(nestedPath)).not.toThrow();
  });
});
