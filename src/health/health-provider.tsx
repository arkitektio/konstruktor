import React, { useEffect, useState } from 'react'
import {
  ContainerState,
  DeadJSON,
  HealthContext,
  HealthReturn,
  HealthyJSON,
  ServiceState,
} from './health-context'
import { fetch } from '@tauri-apps/api/http'

export type IHealthProviderProps = {
  checkInterval?: number
  children: React.ReactNode
}

export type RPContainer = {
  State: string
  Status: string
  Names: string[]
  Image: string
  Labels: { [key: string]: string }
}

export const containerToState = (
  r: RPContainer | undefined
): ContainerState => {
  return {
    state: r?.State || 'down',
    status: r?.Status,
  }
}

export const checkHealth = async (
  name: string,
  host: string,
  port: number
): Promise<HealthReturn> => {
  try {
    //console.log(`Checking ${name} on ${host}:${port}`)
    const res = await fetch(`http://${host}:${port}/ht/?format=json`, {
      headers: {
        'Content-Type': 'application/json',
      },
      method: 'GET',
    })

    if (res.ok)
      return {
        name,
        ok: res.data as HealthyJSON,
        reachable: true,
      }
    else {
      return {
        name,
        error: res.data as DeadJSON,
        reachable: false,
      }
    }
  } catch (e) {
    return {
      name,
      error: { Connection: e as any },
      reachable: false,
    }
  }
}

const HealthProvider: React.FC<IHealthProviderProps> = ({
  children,
  checkInterval,
}) => {
  const [service, setService] = useState<ServiceState>({})

  let host = 'localhost'

  const updateServices = () => {
    checkHealth('arkitekt', host, 8090).then((val) => {
      setService((value) => ({ ...value, arkitekt: val }))
    })
    checkHealth('herre', host, 8000).then((val) =>
      setService((value) => ({ ...value, herre: val }))
    )
    checkHealth('elements', host, 8080).then((val) =>
      setService((value) => ({ ...value, elements: val }))
    )
    checkHealth('fluss', host, 8070).then((val) =>
      setService((value) => ({ ...value, fluss: val }))
    )
    checkHealth('port', host, 8050).then((val) =>
      setService((value) => ({ ...value, port: val }))
    )
  }

  useEffect(() => {
    updateServices()
    const interval = setInterval(() => {
      updateServices()
    }, checkInterval || 3000)
    return () => clearInterval(interval)
  }, [])

  return (
    <HealthContext.Provider value={{ service: service }}>
      {children}
    </HealthContext.Provider>
  )
}

export { HealthProvider }
