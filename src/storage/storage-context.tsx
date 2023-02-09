import React, { useContext } from 'react'

export type App = {
  dirpath: string
  name: string
}

export type StorageContextType = {
  apps: App[]
  installApp: (app: App) => void
}

export const StorageContext = React.createContext<StorageContextType>({
  apps: [],
  installApp: (app) => null,
})

export const useStorage = () => useContext(StorageContext)
