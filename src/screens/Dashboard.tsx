import React, { useEffect, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { useHealth } from '../health/health-context'
import { ResponsiveGrid } from '../layout/ResponsiveGrid'
import { useCommunication } from '../communication/communication-context'
import { Form, Formik } from 'formik'
import { FileField } from './wizard/fields/FileField'
import { stringify } from 'yaml'
import { forage } from '@tauri-apps/tauri-forage'
import type { App } from '../storage/storage-context'
import { useStorage } from '../storage/storage-context'
import { app } from '@tauri-apps/api'
import { FaktsProvider } from '../fakts/fakts-provider'
import { FaktsGuard } from '../fakts/fakts-guard'
import { Health } from './Health'

export enum DockerConnectionStrategy {
  LOCAL = 'LOCAL',
  REMOTE = 'REMOTE',
}

export type DockerConfig = {
  strategy: DockerConnectionStrategy
  addr?: string
}

export type DockerStatus = {
  version: string
  memory: number
}

export const ServiceHealth = (props: { service: any }) => {
  return (
    <ResponsiveGrid>
      {props?.service?.ok &&
        Object.keys(props?.service?.ok).map((key) => (
          <div className='border rounded bg-green-200 border-green-600 bg-white p-3 shadow-xl'>
            <div className='font-light'>{key}</div>
            {JSON.stringify((props?.service?.ok as any)[key])}
          </div>
        ))}
      {props?.service?.error &&
        Object.keys(props?.service?.error).map((key) => (
          <div className='border rounded bg-red-200 border-red-600 bg-white p-3 shadow-xl'>
            <div className='font-light'>{key}</div>
            {JSON.stringify((props?.service?.error as any)[key])}
          </div>
        ))}
    </ResponsiveGrid>
  )
}

export type InitDirectoryValues = {
  dirpath: string
}

export const Dashboard: React.FC<{ app: App }> = ({ app }) => {
  const { call } = useCommunication()
  const [dockerStatus, setDockerStatus] = useState<DockerStatus | null>(null)
  const [advertise, setAdvertise] = useState<boolean>(false)

  useEffect(() => {
    call<DockerConfig, DockerStatus>('test_docker', {
      strategy: DockerConnectionStrategy.LOCAL,
    }).then((res) => setDockerStatus(res))
  }, [])

  const advertiseEndpoint = () => {
    call('advertise_endpoint', {
      docker_addr: 'ssss',
    }).then((res) => console.log(res))
  }

  const test_docker_version = () => {
    call('docker_version_cmd', {
      docker_addr: 'ssss',
    }).then((res) => console.log(res))
  }

  const app_up = () => {
    call('directory_up_cmd', {
      dirpath: app.dirpath,
    }).then((res) => console.log(res))
  }

  const app_stop = () => {
    call('directory_stop_cmd', {
      dirpath: app.dirpath,
    }).then((res) => console.log(res))
  }

  useEffect(() => {
    if (advertise) {
      advertiseEndpoint()
      const interval = setInterval(() => {
        advertiseEndpoint()
      }, 2000 || 3000)
      return () => clearInterval(interval)
    }
    return () => {}
  }, [advertise])

  return (
    <div className='h-full w-full'>
      <div className='text-xl flex flex-row bg-gray-800 text-white shadow-xl mb-2 p-2'>
        <div className='flex-1'>
          <Link to='/'>{'< Home'}</Link>
        </div>
        <div className='flex-grow'>{app.name}</div>
      </div>
      <div className='flex flex-col h-full p-2'>
        <div className='flex flex-row gap-2'>
          <button
            onClick={() => app_up()}
            className='bg-green-500 border border-green-700 p-1 rounded text-white'
          >
            Start{' '}
          </button>
          <button
            onClick={() => app_stop()}
            className='bg-red-400 border border-red-700 p-1 rounded text-white'
          >
            Stop{' '}
          </button>
        </div>
        <div className='font-light mt-2'>Docker Status</div>
        {!!dockerStatus && (
          <div className='font-light text-sm mt-2'>
            DockerVersion: {dockerStatus.version}
            <br />
            Memory Usage: {dockerStatus.memory}
          </div>
        )}
        <div className='font-light mt-2'>Beacon Status</div>
        <button
          onClick={() => setAdvertise(!advertise)}
          className={`border rounded border-gray-300 shadow-xl p-2 mt-2 ${
            advertise ? 'bg-green-300' : 'bg-red-300'
          }`}
        >
          {advertise ? 'Active' : 'Not Active'}
        </button>
        <FaktsProvider
          clientId='PsdU71PlUYeC4hP4aDf8pTdm2Hv9xYKdrxCFI5RO'
          clientSecret='8jXSNhrH7fllN8cGjxg7y2Jl1INb22wlDSmUBepb9aRDGV3al5pfNzswS85MPEvpN5vnfrPkrIERQ6kcMHLiISr4HcYirivdtrnyMjFMlzKGvlCrwfkNJmtQgCLZmH4X'
          store={app.name + 'fakts'}
          endpoint={{ name: app.name, base_url: 'http://localhost:8000/f/' }}
        >
          <FaktsGuard>
            <div className='font-light mt-2'>Status of App</div>
            <Health />
          </FaktsGuard>
        </FaktsProvider>
      </div>
    </div>
  )
}

export const DashboardScreen: React.FC<{}> = (props) => {
  const { id } = useParams<{ id: string }>()
  const { apps } = useStorage()

  let app = apps.find((app) => app.name === id)

  return app ? <Dashboard app={app} /> : <>Could not find this app</>
}
