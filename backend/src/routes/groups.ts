import { Router } from 'express'
import { GroupsController } from '../controllers/groupsController'
import { webhookMiddleware } from '../middleware/webhook'
import { authMiddleware } from '../middleware/auth'

const router = Router()
const controller = new GroupsController()

/**
 * @route   GET /api/groups
 * @desc    List all groups (paginated)
 * @query   page  {number} Page number, 1-indexed (default: 1)
 * @query   limit {number} Items per page, max 100 (default: 20)
 */
router.get('/', controller.listGroups.bind(controller))

// GET /api/groups/:id - Get group by ID
router.get('/:id', controller.getGroup.bind(controller))

// POST /api/groups - Create new group (with webhook) - PROTECTED
router.post('/', authMiddleware, controller.createGroup.bind(controller), webhookMiddleware.afterGroupCreated)

// POST /api/groups/:id/join - Join a group (with webhook) - PROTECTED
router.post('/:id/join', authMiddleware, controller.joinGroup.bind(controller), webhookMiddleware.afterMemberJoined)

// POST /api/groups/:id/contribute - Make contribution (with webhook) - PROTECTED
router.post('/:id/contribute', authMiddleware, controller.contribute.bind(controller), webhookMiddleware.afterContribution)

// GET /api/groups/:id/members - Get group members
router.get('/:id/members', controller.getMembers.bind(controller))

/**
 * @route   GET /api/groups/:id/transactions
 * @desc    Get transactions for a group (paginated)
 * @query   page  {number} Page number, 1-indexed (default: 1)
 * @query   limit {number} Items per page, max 100 (default: 20)
 */// GET /api/groups/:id/transactions - Get group transactions
router.get('/:id/transactions', controller.getTransactions.bind(controller))

export const groupsRouter = router
