export const Logo = ({
  strokeColor = "#000",
  cubeColor = "#5CDECE",
  aColor = "#000",
  width,
  height,
}: {
  strokeColor?: string;
  cubeColor?: string;
  aColor?: string;
  width: number | string;
  height: number | string;
}) => {
  return (
    <>
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width={width}
        height={height}
        fill="none"
        viewBox="0 0 200 200"
      >
        <path
          stroke={strokeColor}
          strokeWidth="8"
          d="M97.488 15.749a6 6 0 0 1 6.024 0l68.5 39.767A6 6 0 0 1 175 60.705v79.59a5.998 5.998 0 0 1-2.988 5.189l-68.719 39.894a5.999 5.999 0 0 1-5.685.183l-68.281-33.977A6 6 0 0 1 26 146.212V60.705a6 6 0 0 1 2.988-5.19l68.5-39.766Z"
        />
        <path stroke={strokeColor} strokeWidth="6" d="m52 121-26 17" />
        <path
          stroke={strokeColor}
          strokeWidth="7"
          d="M173.148 143.188 98.442 100.5"
        />
        <path stroke={strokeColor} strokeWidth="6" d="M100.706 17.37v53.922" />
        <g filter="url(#a)">
          <path
            fill={cubeColor}
            d="M96.569 54.702a10.001 10.001 0 0 1 9.407 0l35.842 19.103a10 10 0 0 1 5.296 8.825v36.863a10 10 0 0 1-5.296 8.825l-36.183 19.285a9.998 9.998 0 0 1-8.86.271l-35.5-16.218a10 10 0 0 1-5.845-9.096V82.63a10 10 0 0 1 5.296-8.825L96.57 54.702Z"
          />
          <path
            stroke={cubeColor}
            strokeWidth="8"
            d="m143.699 70.275-35.842-19.103a14 14 0 0 0-13.17 0L58.845 70.275A14 14 0 0 0 51.43 82.63v39.93a14 14 0 0 0 8.182 12.734l35.502 16.219a14 14 0 0 0 12.402-.38l36.183-19.285a14 14 0 0 0 7.415-12.355V82.63a14 14 0 0 0-7.415-12.355Z"
          />
          <path
            stroke={strokeColor}
            strokeOpacity=".04"
            strokeWidth="8"
            d="m143.699 70.275-35.842-19.103a14 14 0 0 0-13.17 0L58.845 70.275A14 14 0 0 0 51.43 82.63v39.93a14 14 0 0 0 8.182 12.734l35.502 16.219a14 14 0 0 0 12.402-.38l36.183-19.285a14 14 0 0 0 7.415-12.355V82.63a14 14 0 0 0-7.415-12.355Z"
          />
        </g>
        <g>
          <path
            fill={aColor}
            d="M93.727 125.989c-3.34 0-6.318-.58-8.931-1.739-2.614-1.182-4.682-2.92-6.205-5.216-1.5-2.318-2.25-5.204-2.25-8.659 0-2.909.534-5.352 1.602-7.33 1.068-1.977 2.523-3.568 4.364-4.772 1.84-1.205 3.932-2.114 6.272-2.727a47.81 47.81 0 0 1 7.432-1.296c3.046-.318 5.5-.614 7.364-.886 1.864-.296 3.216-.728 4.057-1.296.841-.568 1.261-1.409 1.261-2.522v-.205c0-2.16-.682-3.83-2.045-5.011-1.341-1.182-3.25-1.773-5.728-1.773-2.613 0-4.693.58-6.238 1.739-1.546 1.136-2.568 2.568-3.068 4.295L78.182 87.5c.682-3.182 2.022-5.932 4.022-8.25 2-2.34 4.58-4.136 7.74-5.386 3.181-1.273 6.863-1.91 11.045-1.91 2.909 0 5.693.342 8.352 1.023 2.682.682 5.057 1.739 7.125 3.17a15.402 15.402 0 0 1 4.943 5.523c1.205 2.228 1.807 4.898 1.807 8.012V125h-13.773v-7.261h-.409a14.784 14.784 0 0 1-3.375 4.329c-1.409 1.227-3.102 2.193-5.079 2.898-1.978.682-4.262 1.023-6.853 1.023Zm4.16-10.023c2.136 0 4.022-.421 5.658-1.261 1.637-.864 2.921-2.023 3.853-3.478.932-1.454 1.397-3.102 1.397-4.943v-5.557c-.454.296-1.079.568-1.875.818-.772.228-1.647.444-2.625.648-.977.182-1.954.352-2.931.512-.978.136-1.864.261-2.66.375-1.704.25-3.193.647-4.465 1.193-1.273.545-2.262 1.284-2.966 2.216-.705.909-1.057 2.045-1.057 3.409 0 1.977.716 3.488 2.148 4.534 1.454 1.023 3.295 1.534 5.522 1.534Z"
          />
        </g>
        <defs>
          <filter
            id="a"
            width="109.685"
            height="113.252"
            x="46.43"
            y="44.526"
            colorInterpolationFilters="sRGB"
            filterUnits="userSpaceOnUse"
          >
            <feFlood floodOpacity="0" result="BackgroundImageFix" />
            <feBlend
              in="SourceGraphic"
              in2="BackgroundImageFix"
              result="shape"
            />
            <feGaussianBlur
              result="effect1_foregroundBlur_3_6"
              stdDeviation=".5"
            />
          </filter>
          <filter
            id="b"
            width="62.875"
            height="70.034"
            x="68.341"
            y="63.955"
            colorInterpolationFilters="sRGB"
            filterUnits="userSpaceOnUse"
          >
            <feFlood floodOpacity="0" result="BackgroundImageFix" />
            <feGaussianBlur in="BackgroundImage" stdDeviation="4" />
            <feComposite
              in2="SourceAlpha"
              operator="in"
              result="effect1_backgroundBlur_3_6"
            />
            <feColorMatrix
              in="SourceAlpha"
              result="hardAlpha"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0"
            />
            <feOffset dy="2" />
            <feGaussianBlur stdDeviation="1" />
            <feComposite in2="hardAlpha" operator="out" />
            <feColorMatrix values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.25 0" />
            <feBlend
              in2="effect1_backgroundBlur_3_6"
              result="effect2_dropShadow_3_6"
            />
            <feBlend
              in="SourceGraphic"
              in2="effect2_dropShadow_3_6"
              result="shape"
            />
          </filter>
        </defs>
      </svg>
    </>
  );
};
