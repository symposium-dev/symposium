import { DomBuilder, ExtendedHTMLElement } from '../../../helper/dom';
import { ChatItemButton, FilterOption } from '../../../static';
import testIds from '../../../helper/test-ids';
import { ChatItemFormItemsWrapper } from '../chat-item-form-items';
import { Button } from '../../button';
import { Icon } from '../../icon';
import { OverlayHorizontalDirection, OverlayVerticalDirection } from '../../overlay';

export interface PromptOptionsProps {
  classNames?: string[];
  filterOptions: FilterOption[];
  buttons: ChatItemButton[];
  onFiltersChange?: (filterFormData: Record<string, any>, isValid: boolean) => void;
  onButtonClick?: (buttonId: string) => void;
}

export class PromptOptions {
  render: ExtendedHTMLElement;
  private readonly props: PromptOptionsProps;
  private formItemsWrapper: ChatItemFormItemsWrapper;
  constructor (props: PromptOptionsProps) {
    this.props = props;
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.prompt.options,
      classNames: [ 'mynah-prompt-input-options', ...(this.props.classNames ?? []) ],
      children: this.getFilterOptionsWrapper()
    });
  }

  private readonly getFilterOptionsWrapper = (): Array<ExtendedHTMLElement | string> => {
    let result: Array<ExtendedHTMLElement | string> = [ '' ];
    if (this.props.filterOptions?.length > 0) {
      this.formItemsWrapper = new ChatItemFormItemsWrapper({
        tabId: '',
        chatItem: {
          formItems: this.props.filterOptions
        },
        onFormChange: this.props.onFiltersChange
      });
      result = [
        this.formItemsWrapper.render
      ];
    }
    if (this.props.buttons?.length > 0) {
      this.props.buttons.forEach((button: ChatItemButton) => {
        result.push(new Button({
          onClick: () => {
            this.props.onButtonClick?.(button.id);
          },
          border: false,
          primary: false,
          status: button.status,
          label: button.text,
          disabled: button.disabled,
          tooltip: button.description,
          fillState: 'always',
          tooltipHorizontalDirection: OverlayHorizontalDirection.START_TO_RIGHT,
          tooltipVerticalDirection: OverlayVerticalDirection.TO_TOP,
          ...(button.icon != null ? { icon: new Icon({ icon: button.icon }).render } : {}),
        }).render);
      });
    }

    return result; // [ '' ];
  };

  public readonly update = (filterOptions?: FilterOption[], buttons?: ChatItemButton[]): void => {
    if (filterOptions != null) {
      this.props.filterOptions = filterOptions;
    }
    if (buttons != null) {
      this.props.buttons = buttons;
    }
    this.render.update({
      children: this.getFilterOptionsWrapper()
    });
  };

  public readonly getOptionValues = (): Record<string, string | Array<Record<string, string>>> => {
    return this.formItemsWrapper?.getAllValues() ?? {};
  };
}
