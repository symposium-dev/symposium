import { ChatItem } from '@aws/mynah-ui';
const STREAM_DELAY = 150;
const INITIAL_STREAM_DELAY = 300;

export class Connector {
  requestGenerativeAIAnswer = async (
    streamingChatItems: Array<Partial<ChatItem>>,
    onStreamUpdate: (chatItem: Partial<ChatItem>) => boolean,
    onStreamEnd: () => void
  ): Promise<boolean> => await new Promise((resolve, reject) => {
    setTimeout(() => {
      resolve(true);
      let streamFillInterval: ReturnType<typeof setInterval>;
      const mdStream = streamingChatItems.map(i => i).reverse();
      const intervalTimingMultiplier = Math.floor(Math.random() * (2) + 1);

      const endStream = (): void => {
        onStreamEnd();
        clearInterval(streamFillInterval);
      };
      setTimeout(() => {
        streamFillInterval = setInterval(() => {
          if (mdStream.length > 0) {
            const stopStream = onStreamUpdate(mdStream.pop() ?? {});
            if (stopStream) {
              endStream();
            }
          } else {
            endStream();
          }
        }, STREAM_DELAY * intervalTimingMultiplier);
      }, INITIAL_STREAM_DELAY);
    }, 150);
  });
}
