import React from 'react'
import type { StepProps } from '../types'

export const Done: React.FC<StepProps> = (props) => {
  return (
    <div className='text-center h-full my-7'>
      <div className='font-light text-9xl'> Almost Done!</div>
      <div className='font-light text-3xl mt-4'>
        Please make sure your computer is connected to the internet and all
        unnecessary apps are closed. If you click next the app will be
        installed. Get a coffee ready... ☕
      </div>
    </div>
  )
}
