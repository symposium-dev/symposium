import { Page } from 'playwright/test';
import testIds from '../../src/helper/test-ids';
export const DEFAULT_VIEWPORT = {
  width: 500,
  height: 950
};

export const getOffsetHeight = (boxRect: {
  width?: number;
  height?: number;
  x?: number;
  y?: number;
} | null): number => {
  return boxRect != null ? (boxRect?.y ?? 0) + (boxRect?.height ?? 0) : 0;
};

export const normalizeText = (text: string): string => text.replace(/\r\n/g, '\n').trim();

export const waitForAnimationEnd = async (page: Page): Promise<any> => {
  return await Promise.race([
    new Promise((resolve) => setTimeout(resolve, 10000)),
    page.evaluate(async () => {
      return await new Promise<void>((resolve) => {
        const startTime = new Date().getTime();
        const animationStateCheckInterval: ReturnType<typeof setInterval> = setInterval(() => {
          const allAnims = document.getAnimations();
          if (allAnims.find((anim) => anim.playState !== 'finished') == null || new Date().getTime() - startTime > 6000) {
            clearInterval(animationStateCheckInterval);
            // Give a delay to make the render complete
            setTimeout(() => {
              resolve();
            }, 550);
          }
        }, 550);
      });
    }),
  ]);
};

export async function justWait (duration: number): Promise<void> {
  return await new Promise<void>((resolve) => {
    setTimeout(() => {
      resolve();
    }, duration);
  });
}

export function getSelector (selector: string): string {
  return `css=[${testIds.selector}="${selector}"]`;
}

export function isVisible (el: SVGElement | HTMLElement): boolean {
  const style = window.getComputedStyle(el);
  return style.display !== 'none' && style.visibility !== 'hidden' && style.opacity !== '0';
}
