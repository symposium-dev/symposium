/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../main';
import { ValidationPattern } from '../static';

export const isTextualFormItemValid = (value: string, validationPatterns: {
  operator?: 'and' | 'or';
  genericValidationErrorMessage?: string;
  patterns: ValidationPattern[];
}, mandatory?: boolean): {
  isValid: boolean;
  validationErrors: string[];
} => {
  let isValid = true;
  let validationErrors: string[] = [];
  if (validationPatterns != null) {
    isValid = validationPatterns.patterns.reduce<boolean>((prevValidation, currentPattern): boolean => {
      const isCurrentPatternValid = (value.match(currentPattern.pattern) != null);
      if (!isCurrentPatternValid && currentPattern.errorMessage != null) {
        validationErrors.push(currentPattern.errorMessage);
      }
      if (validationPatterns.operator === 'and') {
        return prevValidation && isCurrentPatternValid;
      }
      return prevValidation || isCurrentPatternValid;
    }, validationPatterns.operator === 'and');
  }
  // Don't invalidate if the field is empty and non mandatory
  isValid = isValid || ((value === undefined || value.trim() === '') && (mandatory !== true));
  if (isValid) {
    validationErrors = [];
  } else if (validationErrors.length === 0 && validationPatterns.genericValidationErrorMessage != null) {
    validationErrors.push(validationPatterns.genericValidationErrorMessage);
  }
  return { isValid, validationErrors };
};

export const isMandatoryItemValid = (value: string): boolean => value !== undefined && value.trim() !== '';

export const checkTextElementValidation = (inputElement: ExtendedHTMLElement, validationPatterns: {
  operator?: 'and' | 'or';
  patterns: ValidationPattern[];
} | undefined, validationErrorBlock: ExtendedHTMLElement, readyToValidate: boolean, mandatory?: boolean): void => {
  const { isValid, validationErrors } = isTextualFormItemValid(inputElement.value, validationPatterns ?? { patterns: [] }, mandatory);
  if (readyToValidate && validationErrors.length > 0 && !isValid) {
    inputElement.addClass('validation-error');
    validationErrorBlock.update({ children: validationErrors.map(message => DomBuilder.getInstance().build({ type: 'span', children: [ message ] })) });
  } else {
    readyToValidate = false;
    validationErrorBlock.clear();
    inputElement.removeClass('validation-error');
  }
};
