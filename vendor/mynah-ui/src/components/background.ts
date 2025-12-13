/* eslint-disable @typescript-eslint/no-extraneous-class */
/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../helper/dom';
import { StyleLoader } from '../helper/style-loader';

export class GradientBackground {
  render: ExtendedHTMLElement;
  constructor () {
    StyleLoader.getInstance().load('components/_background.scss');
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-ui-gradient-background' ],
      innerHTML: `<svg width="478" height="834" viewBox="0 0 478 834" fill="none" xmlns="http://www.w3.org/2000/svg" preserveAspectRatio="xMaxYMax meet">
<g clip-path="url(#clip0_1096_21420)">
<g filter="url(#filter0_f_1096_21420)">
<path d="M33.5412 468.757C55.4273 456.946 246.97 450.5 270.343 416.149C293.715 381.798 340.941 336.254 358.828 378.861C376.715 421.467 365.556 509.607 354.992 530.695C344.429 551.782 223.819 537.162 196.443 568.476C169.068 599.789 55.0403 751.589 10.2557 734.726C-34.5288 717.864 -61.0944 648.254 -57.8245 618.012C-54.5546 587.771 11.6551 480.568 33.5412 468.757Z" fill="var(--mynah-bg-gradient-end)"/>
</g>
<g filter="url(#filter1_f_1096_21420)">
<path d="M175.893 589.032C205.395 579.825 415.137 608.892 457.511 574.467C499.885 540.043 573.574 497.483 571.558 549.079C569.542 600.675 513.444 698.269 491.512 720.12C469.579 741.971 346.848 702.538 301.677 732.766C256.505 762.995 57.668 913.08 17.8124 885.503C-22.0433 857.926 -15.884 774.134 2.76124 740.542C21.4064 706.95 146.39 598.24 175.893 589.032Z" fill="var(--mynah-bg-gradient-next)"/>
</g>
<g filter="url(#filter2_f_1096_21420)">
<path d="M475.23 799.013C504.23 850.513 503.23 994.013 491.23 1018.01C479.23 1042.01 197.23 1070.51 125.23 1068.01C53.2299 1065.51 48.7298 1021.51 53.2298 976.513C57.7298 931.513 132.73 954.513 177.23 958.513C221.73 962.513 287.23 1016.51 301.23 937.013C315.23 857.513 177.23 825.013 177.23 766.513C177.23 708.013 277.73 683.013 324.73 687.513C371.73 692.013 446.23 747.513 475.23 799.013Z" fill="var(--mynah-bg-gradient-mid)"/>
</g>
<g filter="url(#filter3_f_1096_21420)">
<path d="M213.873 750.634C271.012 721.031 296.737 794.04 292.62 825.033C288.503 856.026 382.579 977.946 300.316 993.622C218.054 1009.3 53.6444 1008.82 79.1099 1065C104.576 1121.18 -24.7243 1010.93 -89.8184 979.926C-154.913 948.928 -108.365 847.876 -54.438 864.397C-0.510756 880.919 156.735 780.237 213.873 750.634Z" fill="var(--mynah-bg-gradient-next)"/>
</g>
<g filter="url(#filter4_f_1096_21420)">
<path d="M526.08 514.751C589.306 588.559 550.316 1083.55 550.316 1083.55C550.316 1083.55 526.18 1114.71 481.703 1078.15C437.225 1041.59 347.999 964.645 320.686 840.552C293.374 716.458 309.478 642.394 373.542 502.735C437.606 363.077 462.855 440.943 526.08 514.751Z" fill="var(--mynah-bg-gradient-prev)"/>
</g>
</g>
<defs>
<filter id="filter0_f_1096_21420" x="-244.995" y="174.71" width="800.158" height="748.219" filterUnits="userSpaceOnUse" color-interpolation-filters="linearRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
<feGaussianBlur stdDeviation="93.45" result="effect1_foregroundBlur_1096_21420"/>
</filter>
<filter id="filter1_f_1096_21420" x="-198.68" y="338.179" width="957.177" height="737.589" filterUnits="userSpaceOnUse" color-interpolation-filters="linearRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
<feGaussianBlur stdDeviation="93.45" result="effect1_foregroundBlur_1096_21420"/>
</filter>
<filter id="filter2_f_1096_21420" x="-134.9" y="500.1" width="820.935" height="754.967" filterUnits="userSpaceOnUse" color-interpolation-filters="linearRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
<feGaussianBlur stdDeviation="93.45" result="effect1_foregroundBlur_1096_21420"/>
</filter>
<filter id="filter3_f_1096_21420" x="-308.51" y="556.81" width="829.574" height="710.795" filterUnits="userSpaceOnUse" color-interpolation-filters="linearRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
<feGaussianBlur stdDeviation="93.45" result="effect1_foregroundBlur_1096_21420"/>
</filter>
<filter id="filter4_f_1096_21420" x="121.038" y="238.468" width="628.069" height="1044.36" filterUnits="userSpaceOnUse" color-interpolation-filters="linearRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
<feGaussianBlur stdDeviation="93.45" result="effect1_foregroundBlur_1096_21420"/>
</filter>
<clipPath id="clip0_1096_21420">
<rect width="478" height="834" fill="white"/>
</clipPath>
</defs>
</svg>`
    });
  }
}
