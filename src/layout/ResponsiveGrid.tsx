import React, { Children } from 'react'

export type IResponsiveGridProps = {
  children: React.ReactNode
}

const ResponsiveGrid: React.FC<IResponsiveGridProps> = ({ children }) => {
  return (
    <div className='pt-2 pb-2 pr-2 grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 xl:grid-cols-5 gap-4'>
      {children}
    </div>
  )
}

export { ResponsiveGrid }
