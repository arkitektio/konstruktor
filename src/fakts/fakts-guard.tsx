import React, { useState } from 'react'
import { useFakts } from './fakts-config'

export const FaktsGuard: React.FC<{
  key?: string
  fallback?: React.ReactNode
  children: React.ReactNode
}> = ({ key, children, fallback }) => {
  const { fakts } = useFakts()

  if (key) {
    if (fakts && fakts[key]) {
      return <>{children}</>
    } else {
      return <>{fallback || `Did not find fakts for ${key}`}</>
    }
  }

  if (fakts) return <>{children}</>

  return <>{fallback || `Did not find fakts!`}</>
}
