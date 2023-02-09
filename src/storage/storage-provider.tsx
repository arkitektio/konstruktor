import { forage } from '@tauri-apps/tauri-forage'
import React, { useEffect, useState } from 'react'
import { App, StorageContext } from './storage-context'

const StorageProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [availableApps, setAvailableApps] = useState<App[]>([])

  const installApp = async (app: App) => {
    let newapps = [...availableApps, app]

    await forage.setItem({
      key: 'installed_apps',
      value: JSON.stringify(newapps),
    })()
    setAvailableApps(newapps)
  }

  useEffect(() => {
    forage
      .getItem({ key: 'installed_apps' })()
      .then((value) => {
        console.log(value)
        setAvailableApps(JSON.parse(value) || [])
      })
  }, [])

  return (
    <StorageContext.Provider
      value={{ apps: availableApps, installApp: installApp }}
    >
      {children}
    </StorageContext.Provider>
  )
}

export { StorageProvider }
