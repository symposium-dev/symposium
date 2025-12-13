/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

export const getTimeDiff = (differenceInMs: number, show?: {
  years?: boolean;
  months?: boolean;
  weeks?: boolean;
  days?: boolean;
  hours?: boolean;
  minutes?: boolean;
} | 1 | 2 | 3 | 4 | 5 | 6, separator?: string): string => {
  // get total seconds for the difference
  let delta = Math.abs(differenceInMs) / 1000;

  // calculate (and subtract) whole years
  const years = Math.floor(delta / (86_400 * 30 * 12));
  delta -= years * (86_400 * 30 * 12);

  // calculate (and subtract) whole months
  const months = Math.floor(delta / (86_400 * 30));
  delta -= months * (86_400 * 30);

  // calculate (and subtract) whole weeks
  const weeks = Math.floor(delta / (86_400 * 7));
  delta -= weeks * (86_400 * 7);

  // calculate (and subtract) whole days
  const days = Math.floor(delta / 86_400);
  delta -= days * 86_400;

  // calculate (and subtract) whole hours
  const hours = Math.floor(delta / 3_600) % 24;
  delta -= hours * 3_600;

  // calculate (and subtract) whole minutes
  const minutes = Math.floor(delta / 60) % 60;
  delta -= minutes * 60;

  const combined = [];
  if (years !== 0 && (show === undefined || typeof show !== 'object' || show.years !== false)) {
    combined.push(`${years}yr`);
  }
  if (months !== 0 && (show === undefined || typeof show !== 'object' || show.months !== false)) {
    combined.push(`${months}mo`);
  }
  if (weeks !== 0 && (show === undefined || typeof show !== 'object' || show.weeks !== false)) {
    combined.push(`${weeks}we`);
  }
  if (days !== 0 && (show === undefined || typeof show !== 'object' || show.days !== false)) {
    combined.push(`${days}da`);
  }
  if (hours !== 0 && (show === undefined || typeof show !== 'object' || show.hours !== false)) {
    combined.push(`${hours}hr`);
  }
  if (minutes !== 0 && (show === undefined || typeof show !== 'object' || show.minutes !== false)) {
    combined.push(`${minutes}min`);
  }

  if (years + months + weeks + days + hours + minutes === 0) {
    return '1min';
  }

  if (show !== undefined && typeof show === 'number') {
    combined.splice(show, combined.length);
  }
  return combined.join(separator ?? ' ');
};
