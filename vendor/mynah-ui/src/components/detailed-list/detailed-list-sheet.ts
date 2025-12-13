import { MynahUIGlobalEvents } from '../../helper/events';
import { ChatItemButton, DetailedList, DetailedListItem, MynahEventNames } from '../../static';
import { SheetProps } from '../sheet';
import { DetailedListWrapper } from './detailed-list';

export interface DetailedListSheetProps {
  tabId?: string; // TODO: remove this in new major version, still here for backwards compatibility
  detailedList: DetailedList;
  events?: {
    onFilterValueChange?: (filterValues: Record<string, any>, isValid: boolean) => void;
    onKeyPress?: (e: KeyboardEvent) => void;
    onItemSelect?: (detailedListItem: DetailedListItem) => void;
    onItemClick?: (detailedListItem: DetailedListItem) => void;
    onBackClick?: () => void;
    onTitleActionClick?: (action: ChatItemButton) => void;
    onActionClick?: (action: ChatItemButton, listItem?: DetailedListItem) => void;
    onFilterActionClick?: (action: ChatItemButton, filterValues?: Record<string, any>, isValid?: boolean) => void;
    onClose?: () => void;
  };
}

export class DetailedListSheet {
  props: DetailedListSheetProps;
  detailedListWrapper: DetailedListWrapper;
  private readonly keyPressHandler: (e: KeyboardEvent) => void;

  constructor (props: DetailedListSheetProps) {
    this.props = props;
    // To prevent the header from being shown in the detailed list wrapper
    const detailedListCopy: DetailedList = { ...props.detailedList, header: undefined };
    this.detailedListWrapper = new DetailedListWrapper({
      detailedList: detailedListCopy,
      onFilterValueChange: props.events?.onFilterValueChange,
      onItemSelect: props.events?.onItemSelect,
      onItemClick: props.events?.onItemClick,
      onItemActionClick: props.events?.onActionClick,
      onFilterActionClick: props.events?.onFilterActionClick,
    });
    this.keyPressHandler = (e: KeyboardEvent) => {
      this.props.events?.onKeyPress?.(e);
    };
  }

  open = (showBackButton?: boolean): void => {
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.OPEN_SHEET, {
      fullScreen: true,
      title: this.props.detailedList.header?.title,
      description: this.props.detailedList.header?.description,
      status: this.props.detailedList.header?.status,
      actions: this.props.detailedList.header?.actions,
      children: [ this.detailedListWrapper.render ],
      showBackButton,
      onClose: () => {
        this.props.events?.onClose?.();
        window.removeEventListener('keydown', this.keyPressHandler);
      },
      onActionClick: (action: ChatItemButton) => {
        this.props.events?.onTitleActionClick?.(action);
      },
      onBack: () => {
        this.props.events?.onBackClick?.();
      }
    } satisfies Partial<SheetProps>);

    window.addEventListener('keydown', this.keyPressHandler);
  };

  update = (detailedList: DetailedList, showBackButton?: boolean): void => {
    this.props.detailedList = { ...this.props.detailedList, ...detailedList };
    if (detailedList.header != null) {
      MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.UPDATE_SHEET, {
        title: this.props.detailedList.header?.title,
        description: this.props.detailedList.header?.description,
        status: this.props.detailedList.header?.status,
        showBackButton,
        actions: this.props.detailedList.header?.actions
      } satisfies Partial<SheetProps>);
    }
    this.detailedListWrapper.update({ ...this.props.detailedList, header: undefined });
  };

  close = (): void => {
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.CLOSE_SHEET);
  };
}
