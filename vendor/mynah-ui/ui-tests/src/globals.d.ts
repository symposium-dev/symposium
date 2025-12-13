import { DetailedList, MynahUI } from '@aws/mynah-ui';

declare global {
  interface Window {
    mynahUI: MynahUI;
    topBarOverlayController: {
      update: (data: DetailedList) => void;
      close: () => void;
    };
  }
}
