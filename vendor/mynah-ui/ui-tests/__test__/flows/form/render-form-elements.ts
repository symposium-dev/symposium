import { expect, Page } from 'playwright/test';
import { waitForAnimationEnd } from '../../helpers';

export const renderFormElements = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });

      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as any,
        snapToTop: true,
        body: `Can you help us to improve our AI Assistant? Please fill the form below and hit **Submit** to send your feedback.

_To send the form, mandatory items should be filled._`,
        formItems: [
          {
            id: 'expertise-area',
            type: 'select',
            title: 'Area of expertise',
            options: [
              {
                label: 'Frontend',
                value: 'frontend',
              },
              {
                label: 'Backend',
                value: 'backend',
              },
              {
                label: 'Data Science',
                value: 'datascience',
              },
              {
                label: 'Other',
                value: 'other',
              },
            ],
          },
          {
            id: 'preferred-ide',
            type: 'radiogroup',
            title: 'Preferred IDE',
            options: [
              {
                label: 'VSCode',
                value: 'vscode',
              },
              {
                label: 'JetBrains IntelliJ',
                value: 'intellij',
              },
              {
                label: 'Visual Studio',
                value: 'visualstudio',
              },
            ],
          },
          {
            id: 'remote-ide',
            type: 'toggle',
            value: 'remote',
            title: 'Environment',
            options: [
              {
                label: 'Remote',
                value: 'remote',
                icon: 'star'
              },
              {
                label: 'Local',
                value: 'local',
                icon: 'scroll-down'
              },
              {
                label: 'Both',
                value: 'both',
                icon: 'stack'
              }
            ],
          },
          {
            id: 'is-online',
            type: 'checkbox',
            value: 'true',
            label: 'Yes',
            title: 'Are you working online?',
          },
          {
            id: 'is-monorepo',
            type: 'switch',
            label: 'Yes',
            icon: 'deploy',
            title: 'Are you working in a monorepo project?',
            tooltip: "If you're working more on monorepos, check this"
          },
          {
            id: 'working-hours',
            type: 'numericinput',
            title: 'How many hours are you using an IDE weekly?',
            placeholder: 'IDE working hours',
          },
          {
            id: 'email',
            type: 'email',
            mandatory: true,
            title: 'Email',
            placeholder: 'email',
          },
          {
            id: 'name',
            type: 'textinput',
            mandatory: true,
            title: 'Name',
            placeholder: 'Name and Surname',
          },
          {
            id: 'ease-of-usage-rating',
            type: 'stars',
            mandatory: true,
            title: 'How easy is it to use our AI assistant?',
          },
          {
            id: 'accuracy-rating',
            type: 'stars',
            mandatory: true,
            title: 'How accurate are the answers you get from our AI assistant?',
          },
          {
            id: 'general-rating',
            type: 'stars',
            title: 'How do feel about our AI assistant in general?',
          },
          {
            id: 'description',
            type: 'textarea',
            title: 'Any other things you would like to share?',
            placeholder: 'Write your feelings about our tool',
          },
        ],
        buttons: [
          {
            id: 'submit',
            text: 'Submit',
            status: 'primary',
          },
          {
            id: 'cancel-feedback',
            text: 'Cancel',
            keepCardAfterClick: false,
            waitMandatoryFormItems: false,
          },
        ],
      });
    }
  });
  await waitForAnimationEnd(page);
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }
};
