import { DomBuilder, ExtendedHTMLElement } from '../../../../helper/dom';
import testIds from '../../../../helper/test-ids';
import { DetailedList, DetailedListItem, ChatItemButton } from '../../../../static';
import { Button } from '../../../button';
import { DetailedListWrapper } from '../../../detailed-list/detailed-list';
import { Icon } from '../../../icon';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from '../../../overlay';

export interface TopBarButtonOverlayProps {
  tabId: string;
  topBarButtonOverlay: DetailedList;
  events?: {
    onKeyPress?: (e: KeyboardEvent) => void;
    onGroupClick?: (groupName: string) => void;
    onItemClick?: (detailedListItem: DetailedListItem) => void;
    onClose?: () => void;
  };
}

export interface TopBarButtonProps {
  topBarButton?: ChatItemButton;
  onTopBarButtonClick?: (action: ChatItemButton) => void;
}

export class TopBarButton {
  render: ExtendedHTMLElement;
  private readonly props: TopBarButtonProps;
  private overlay?: Overlay;
  private checklistSelectorContainer: DetailedListWrapper;
  private overlayData: TopBarButtonOverlayProps;
  private topBarButton: Button;
  private keyPressHandler: (e: KeyboardEvent) => void;

  constructor (props: TopBarButtonProps) {
    this.props = props;

    this.render = DomBuilder.getInstance().build({
      testId: testIds.prompt.topBarButton,
      type: 'span',
      children: this.getTopBarButtonChildren(),
      classNames: [ 'top-bar-button' ],
      attributes: {
        contenteditable: 'false'
      },
    });
  }

  update (newProps: TopBarButtonProps): void {
    if (newProps.topBarButton != null) {
      this.props.topBarButton = newProps.topBarButton;
    }
    this.render.update({
      children: this.getTopBarButtonChildren(),
    });
  }

  closeOverlay (): void {
    this.overlay?.close();
  }

  showOverlay (topBarButtonOverlay: TopBarButtonOverlayProps): void {
    this.overlayData = topBarButtonOverlay;

    if (this.overlay == null) {
      this.keyPressHandler = (e: KeyboardEvent): void => {
        this.overlayData.events?.onKeyPress?.(e);
      };

      this.overlay = new Overlay({
        testId: testIds.prompt.topBarActionOverlay,
        background: true,
        closeOnOutsideClick: true,
        referenceElement: this.topBarButton.render,
        dimOutside: false,
        onClose: () => {
          this.overlay = undefined; this.overlayData.events?.onClose?.(); window.removeEventListener('keydown', this.keyPressHandler);
        },
        removeOtherOverlays: true,
        verticalDirection: OverlayVerticalDirection.TO_TOP,
        horizontalDirection: OverlayHorizontalDirection.END_TO_LEFT,
        children: [ this.getItemGroups() ]
      });
      window.addEventListener('keydown', this.keyPressHandler);
    } else {
      this.overlay.updateContent([ this.getItemGroups() ]);
    }
  }

  getTopBarButtonChildren (): Array<string | ExtendedHTMLElement> {
    this.topBarButton = new Button({
      onClick: () => {
        if (this.props.topBarButton != null) this.props.onTopBarButtonClick?.(this.props.topBarButton);
      },
      primary: false,
      status: 'clear',
      border: false,
      icon: this.props.topBarButton?.icon ? new Icon({ icon: this.props.topBarButton.icon }).render : undefined,
      label: this.props.topBarButton?.text ?? '',
      hidden: this.props.topBarButton == null
    });

    return [ this.topBarButton.render ];
  }

  private readonly getItemGroups = (): ExtendedHTMLElement => {
    if (this.checklistSelectorContainer == null) {
      this.checklistSelectorContainer = new DetailedListWrapper({
        detailedList: this.overlayData.topBarButtonOverlay,
        onGroupClick: this.overlayData.events?.onGroupClick,
        onGroupActionClick: (_, groupName) => { if (groupName != null) this.overlayData.events?.onGroupClick?.(groupName); },
        onItemClick: this.overlayData.events?.onItemClick,
        onItemActionClick: (_, detailedListItem) => { if (detailedListItem != null) this.overlayData.events?.onItemClick?.(detailedListItem); },
      });
    } else {
      this.checklistSelectorContainer?.update(this.overlayData.topBarButtonOverlay, true);
    }

    return DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-chat-prompt-quick-picks-overlay-wrapper' ],
      children: [ this.checklistSelectorContainer.render ]
    });
  };

  onTopBarButtonOverlayChanged (topBarButtonOverlay: DetailedList): void {
    this.overlayData.topBarButtonOverlay = topBarButtonOverlay;
    if (this.overlay != null) {
      this.overlay.updateContent([ this.getItemGroups() ]);
    }
  }
}
